"""Bake Snowballz 2 Blender sources into compact vertex-animation models.

Run from the repo root (requires Blender 4+):

    blender -b art/source/penguin.blend --python tools/export_models.py -- penguin
    blender -b art/source/tree.blend    --python tools/export_models.py -- tree

Any model can replace one of the built-in (meshgen) ones: export it under
that model's file name and the game picks it up (see `prop` below).

    blender -b --python tools/export_models.py -- prop art/sauna.blend sauna
    blender -b --python tools/export_models.py -- prop kit/tower.glb tower --size 3.2

Every animation is baked to N frames of evaluated geometry (armatures,
lattices and subdivision applied), so the game needs no skinning code.

File format (little endian), see crates/client/src/model.rs:
    b"SBM1" u32 n_anims
    per anim:
        u8 name_len, name
        u32 n_verts, u32 n_indices, u32 n_frames
        n_verts * [r, g, b, flags] u8      flags: 1 team colour, 2 glow, 4 gloss,
                                           bits 3-5 material (snow, wood, ...)
        n_indices * u16
        n_frames * n_verts * (pos f32x3, normal i8x3 + pad)
"""

import math
import struct
import sys

import bmesh
import bpy
from mathutils import Matrix, Quaternion, Vector

OUT_DIR = "assets/models"

# Materials painted with the owning player's color.
TEAM_MATERIALS = {"Cloth", "Cloth.002"}
# Surface materials, in the game's order (snow 1, wood 2, ... ice 6).
MATERIAL_WORDS = [("snow",), ("wood", "plank", "log"), ("stone", "rock"), ("cloth", "team", "fabric"), ("metal", "iron", "steel"), ("ice",)]
# Override colors for node-based materials whose base color isn't meaningful.
COLOR_OVERRIDE = {
    "Puple": (0.02, 0.02, 0.03),
    "Penguin Dark": (0.05, 0.055, 0.07),
    "Hammerhead": (0.25, 0.25, 0.27),
    "Snow": (0.95, 0.97, 1.0),
    "Fish": (0.45, 0.55, 0.62),
    "FishBelly": (0.85, 0.82, 0.7),
}


def material_color(mat):
    if mat is None:
        return (0.8, 0.8, 0.8), 0
    flags = 1 if mat.name in TEAM_MATERIALS else prop_flags(mat)
    if mat.name in COLOR_OVERRIDE:
        return COLOR_OVERRIDE[mat.name], flags
    if mat.node_tree:
        for n in mat.node_tree.nodes:
            if n.type == "BSDF_PRINCIPLED":
                return tuple(n.inputs["Base Color"].default_value[:3]), flags
            if n.type == "BSDF_DIFFUSE":
                return tuple(n.inputs["Color"].default_value[:3]), flags
    return tuple(mat.diffuse_color[:3]), flags


def linear_to_srgb(c):
    if c <= 0.0031308:
        return c * 12.92
    return 1.055 * (c ** (1 / 2.4)) - 0.055


def show_everything():
    def walk(lc):
        lc.exclude = False
        lc.hide_viewport = False
        for ch in lc.children:
            walk(ch)

    walk(bpy.context.view_layer.layer_collection)
    for c in bpy.data.collections:
        c.hide_viewport = False
        c.hide_render = False
    for o in bpy.context.scene.objects:
        o.hide_viewport = False
        o.hide_set(False)


class Baker:
    """Collects triangles from a fixed set of objects across frames."""

    def __init__(self, objects, origin, scale):
        self.objects = objects
        self.origin = origin
        self.scale = scale
        self.keys = None  # per object: list of corner keys
        self.colors = []
        self.indices = []
        self.frames = []

    def _evaluated(self, obj):
        dg = bpy.context.evaluated_depsgraph_get()
        ev = obj.evaluated_get(dg)
        mesh = ev.to_mesh()
        mesh.calc_loop_triangles()
        return ev, mesh

    def capture(self):
        first = self.keys is None
        if first:
            self.keys = []
            vert_index = {}
        positions = {}
        for oi, obj in enumerate(self.objects):
            ev, mesh = self._evaluated(obj)
            mw = ev.matrix_world
            nm = mw.to_3x3().inverted_safe().transposed()
            mats = obj.material_slots
            if first and "--parts" in sys.argv:
                print(f"    {obj.name}: {len(mesh.loop_triangles)} tris")
            corner_normals = [cn.vector.copy() for cn in mesh.corner_normals]
            if first:
                obj_keys = []
                for tri in mesh.loop_triangles:
                    mi = tri.material_index
                    mat = mats[mi].material if mi < len(mats) else None
                    for li in tri.loops:
                        vi = mesh.loops[li].vertex_index
                        key = (oi, vi, mi, tri.use_smooth and True or li)
                        obj_keys.append((key, li, vi))
                        if key not in vert_index:
                            vert_index[key] = len(self.colors)
                            rgb, flags = material_color(mat)
                            self.colors.append(
                                (*[round(linear_to_srgb(c) * 255) for c in rgb], flags)
                            )
                        self.indices.append(vert_index[key])
                self.keys.append([(vert_index[k], li, vi) for k, li, vi in obj_keys])
            for idx, li, vi in self.keys[oi]:
                if idx in positions:
                    continue
                p = mw @ mesh.vertices[vi].co
                n = (nm @ corner_normals[li]).normalized()
                p = (p - self.origin) * self.scale
                positions[idx] = (p, n)
            ev.to_mesh_clear()
        frame = [positions[i] for i in range(len(self.colors))]
        self.frames.append(frame)

    def encode(self, name):
        n_verts = len(self.colors)
        assert n_verts < 65536, f"{name}: too many verts {n_verts}"
        frames = self.frames
        # Drop a trailing frame that duplicates the first (looping strips).
        if len(frames) > 2:
            d = max((a[0] - b[0]).length for a, b in zip(frames[0], frames[-1]))
            if d < 1e-4:
                frames = frames[:-1]
        out = bytearray()
        nb = name.encode()
        out += struct.pack("<B", len(nb)) + nb
        out += struct.pack("<III", n_verts, len(self.indices), len(frames))
        for c in self.colors:
            out += struct.pack("<4B", *c)
        out += struct.pack(f"<{len(self.indices)}H", *self.indices)
        for f in frames:
            for p, n in f:
                q = [max(-127, min(127, round(v * 127))) for v in n]
                out += struct.pack("<3f3bx", p.x, p.y, p.z, *q)
        print(f"  {name}: {n_verts} verts, {len(self.indices)//3} tris, {len(frames)} frames")
        return bytes(out)


def write_model(path, anims):
    out = bytearray(b"SBM1")
    out += struct.pack("<I", len(anims))
    for a in anims:
        out += a
    with open(path, "wb") as f:
        f.write(out)
    print(f"wrote {path} ({len(out)} bytes)")


# --------------------------------------------------------------------------
# Penguins

PENGUIN_SCALE = 0.24  # Blender penguin is ~3.8 units tall; a tile is 1 unit.

# name -> (rig, nla tracks, props visible)
PENGUIN_ANIMS = {
    "worker": {
        "stand": (["Stand"], []),
        "walk": (["Walk"], []),
        "carry": (["Walk", "Carry"], ["CarryFish"]),
        "fish": (["Stand", "Chop"], ["Fishing Pole"]),
        "unload": (["Bend", "Unload Tree"], ["UnloadFish"]),
        # Builders swing a hammer, choppers an axe, and a cold worker sits
        # by the fire. "Harvest Tree" is an action with no NLA track of its
        # own, so it gets one (see `add_action_track`).
        "hammer": (["Stand", "Hammer"], ["Hammer Handle", "Hammer Head"]),
        "chop": (["Stand", "Harvest Tree"], ["Axe Handle", "Cube"]),
        "sit": (["Sit"], []),
        "cheer": (["Stand"], []),
        "flinch": (["Stand"], []),
    },
    "warrior": {
        "stand": (["Stand"], []),
        "walk": (["Walk"], []),
        "throw": (["Throw"], ["Snowball.001"]),
        "scoop": (["Bend"], ["Snowball"]),
        "cheer": (["Stand"], []),
        "flinch": (["Stand"], []),
    },
}


def make_fish_mesh():
    """A chunky low-poly fish to carry around instead of a log."""
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=10, v_segments=6, radius=1.0)
    for v in bm.verts:
        v.co.x *= 1.1
        v.co.y *= 0.35
        v.co.z *= 0.45
    # Tail
    t0 = bm.verts.new((-1.0, 0, 0))
    t1 = bm.verts.new((-1.7, 0.05, 0.55))
    t2 = bm.verts.new((-1.7, 0.05, -0.55))
    t3 = bm.verts.new((-1.7, -0.05, 0.55))
    t4 = bm.verts.new((-1.7, -0.05, -0.55))
    bm.faces.new((t0, t1, t2))
    bm.faces.new((t0, t4, t3))
    bm.faces.new((t1, t3, t4, t2))
    for f in bm.faces:
        f.smooth = len(f.verts) > 3
        f.material_index = 1 if f.calc_center_median().z < -0.15 else 0
    me = bpy.data.meshes.new("FishMesh")
    bm.to_mesh(me)
    for name, rgb in (("Fish", COLOR_OVERRIDE["Fish"]), ("FishBelly", COLOR_OVERRIDE["FishBelly"])):
        m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
        me.materials.append(m)
    return me


def setup_fish_props(scene):
    fish = make_fish_mesh()
    # Re-use the log objects' bone parenting but swap in a fish.
    for log_name, prop_name in (("Log.001", "CarryFish"), ("Log.003", "UnloadFish")):
        log = scene.objects[log_name]
        log.data = fish
        log.scale = (0.75, 0.75, 0.75)
        log.name = prop_name


def _rot(rig, bone, axis, deg):
    """Set a pose bone to a turn of `deg` degrees about one local axis."""
    rig.pose.bones[bone].rotation_quaternion = Quaternion(axis, math.radians(deg))


def pose_cheer(rig, f):
    """Both arms thrown up and a little hop, looping over six frames."""
    t = f / 6 * math.tau
    lift = 128 + 22 * math.sin(t)
    _rot(rig, "upper_arm.L", (0, 0, 1), lift)
    _rot(rig, "upper_arm.R", (0, 0, 1), -lift)
    root = rig.pose.bones["spine"]
    root.location = (0.0, 0.5 * abs(math.sin(t / 2)), 0.0)  # local Y is up
    _rot(rig, "spine.002", (1, 0, 0), -6 * math.sin(t))


def pose_flinch(rig, f):
    """Rocked back by a hit, arms flung out, then recovering (not a loop)."""
    amount = [0.0, 0.8, 1.0, 0.7, 0.35, 0.1][min(f, 5)]
    rig.pose.bones["spine"].location = (0.0, 0.0, 0.0)
    _rot(rig, "spine.001", (1, 0, 0), -17 * amount)
    _rot(rig, "spine.002", (1, 0, 0), -8 * amount)
    arm = -48 + 88 * amount  # from the resting -48 degrees out to +40
    _rot(rig, "upper_arm.L", (0, 0, 1), arm)
    _rot(rig, "upper_arm.R", (0, 0, 1), -arm)


# Animations made by posing bones in code: the source rig has no clip for
# them. Each is applied on top of the standing pose, after the frame is set.
PROCEDURAL = {"cheer": pose_cheer, "flinch": pose_flinch}


def add_action_track(rig, name):
    """An NLA track that plays the action called `name`, if there is none yet."""
    if name in rig.animation_data.nla_tracks:
        return
    action = bpy.data.actions.get(name)
    if action is None:
        return
    track = rig.animation_data.nla_tracks.new()
    track.name = name
    strip = track.strips.new(name, 0, action)
    # Blender 4.4+ actions are slotted: bind the strip to the rig's slot.
    try:
        strip.action_slot = action.slots[0]
    except (AttributeError, IndexError):
        pass
    track.mute = True


def set_pose(rig, tracks):
    for t in rig.animation_data.nla_tracks:
        t.mute = not (t.name == "Base" or t.name in tracks)
        for s in t.strips:
            if not t.mute and t.name != "Base":
                # Some strips have zero influence keyed; force full weight.
                s.use_animated_influence = False
                s.influence = 1.0


def outfit_action(arm, anim_tracks):
    """Outfit armatures (Toga, Stocking Cap) have per-pose actions."""
    if not arm.animation_data:
        return
    for t in arm.animation_data.nla_tracks:
        t.mute = True
    pose = "Walk" if "Walk" in anim_tracks else "Stand"
    for cand in (f"{arm.name} {pose}", f"{arm.name} Stand", f"{arm.name} Base"):
        if cand in bpy.data.actions:
            arm.animation_data.action = bpy.data.actions[cand]
            return


# Triangles to keep, by object name prefix, as a share of what the object has
# after subdivision. The eyes were over half of a penguin: a lid alone was
# 1,384 triangles. Anything unlisted keeps its geometry.
LITE_RATIOS = {"Lid": 0.07, "Ball": 0.3, "Puple": 0.3, "Stocking Cap": 0.3, "Snowball": 0.15}


def thicken_tools(scene):
    """Tool handles are about 0.4 across on a 4-unit penguin: a pixel wide
    at game scale, so the axe head seems to float. Fatten them."""
    for name in ("Axe Handle", "Hammer Handle"):
        o = scene.objects.get(name)
        if o is None or any(m.name == "fat" for m in o.modifiers):
            continue
        m = o.modifiers.new("fat", "SOLIDIFY")
        m.thickness = 0.55
        m.offset = 0.0


def lighten(scene):
    """Cut a penguin from about 7,500 triangles to about 2,000, so a few
    thousand can be on screen at once (and phones can draw them)."""
    for o in scene.objects:
        if o.type != "MESH":
            continue
        for prefix, ratio in LITE_RATIOS.items():
            if o.name.startswith(prefix):
                # First in the stack, so every frame has the same topology.
                d = o.modifiers.new("lite", "DECIMATE")
                d.ratio = ratio
                while o.modifiers[0] != d:
                    o.modifiers.move(len(o.modifiers) - 1, 0)
        subs = [m for m in o.modifiers if m.type == "SUBSURF"]
        # The body, feet, apron, cap and toga read fine on their base meshes.
        if o.name.startswith(("Body", "Foot", "Apron", "Toga", "Stocking Cap", "Flat Stocking")):
            for m in subs:
                m.levels = 0
                m.render_levels = 0


def simplify(scene):
    """Trim subdivision on accessories so a penguin is a few thousand tris."""
    for o in scene.objects:
        if o.type != "MESH":
            continue
        subs = [m for m in o.modifiers if m.type == "SUBSURF"]
        if o.name.startswith(("Body", "Snowball")):
            continue
        if o.name.startswith("Stocking Cap"):
            for i, m in enumerate(subs):
                m.levels = 1 if i == 0 else 0
        else:
            for m in subs:
                m.levels = 0
        if o.name.startswith("Lid"):
            for m in [m for m in o.modifiers if m.type == "SOLIDIFY"]:
                o.modifiers.remove(m)


def export_penguin():
    scene = bpy.context.scene
    show_everything()
    simplify(scene)
    if "--full" not in sys.argv:
        lighten(scene)
    setup_fish_props(scene)
    add_action_track(scene.objects["Worker Metarig"], "Harvest Tree")
    thicken_tools(scene)

    outfits = {
        "worker": ["Stocking Cap.001"],
        "warrior": ["Toga Mesh"],
    }
    bodies = {
        "worker": [o.name for o in bpy.data.collections["Meshes"].objects],
        "warrior": [o.name for o in bpy.data.collections["Warrior Penguin Meshes"].objects],
    }
    rigs = {"worker": "Worker Metarig", "warrior": "Warrior Metarig"}
    outfit_rigs = {"worker": "Stocking Cap", "warrior": "Toga"}

    for kind, anims in PENGUIN_ANIMS.items():
        rig = scene.objects[rigs[kind]]
        origin = rig.matrix_world.translation.copy()
        encoded = []
        for anim, (tracks, props) in anims.items():
            set_pose(rig, tracks)
            orig = scene.objects.get(outfit_rigs[kind])
            if orig:
                outfit_action(orig, tracks)
            names = bodies[kind] + outfits[kind] + props
            objs = [scene.objects[n] for n in names]
            baker = Baker(objs, origin, PENGUIN_SCALE)
            for f in range(0, 6):
                scene.frame_set(f)
                if anim in PROCEDURAL:
                    PROCEDURAL[anim](rig, f)
                    bpy.context.view_layer.update()
                baker.capture()
            encoded.append(baker.encode(anim))
        write_model(f"{OUT_DIR}/{kind}.sbm", encoded)


# --------------------------------------------------------------------------
# Trees


def export_tree():
    scene = bpy.context.scene
    show_everything()
    objs = [scene.objects["tree1"], scene.objects["treetrunk1"]]
    # There can be ~800 trees on a map; keep them light.
    for o, ratio in zip(objs, (0.08, 0.15)):
        d = o.modifiers.new("decimate", "DECIMATE")
        d.ratio = ratio
    baker = Baker(objs, Vector((0, 0, 0)), 0.42)
    baker.capture()
    write_model(f"{OUT_DIR}/tree.sbm", [baker.encode("tree")])


# --------------------------------------------------------------------------
# Props: any static model standing in for a meshgen one


def _arg(flag, default):
    return float(sys.argv[sys.argv.index(flag) + 1]) if flag in sys.argv else default


def load_source(path):
    """Open a .blend, or import a .glb/.gltf/.fbx/.obj into an empty scene."""
    if path.endswith(".blend"):
        bpy.ops.wm.open_mainfile(filepath=path)
        return
    bpy.ops.wm.read_factory_settings(use_empty=True)
    ext = path.rsplit(".", 1)[-1].lower()
    if ext in ("glb", "gltf"):
        bpy.ops.import_scene.gltf(filepath=path)
    elif ext == "fbx":
        bpy.ops.import_scene.fbx(filepath=path)
    elif ext == "obj":
        bpy.ops.wm.obj_import(filepath=path)
    else:
        sys.exit(f"don't know how to read {path}")


def base_image(mat):
    """The image feeding a material's base color, if it is textured."""
    if mat is None or not mat.node_tree:
        return None
    for n in mat.node_tree.nodes:
        if n.type == "BSDF_PRINCIPLED":
            for link in n.inputs["Base Color"].links:
                if link.from_node.type == "TEX_IMAGE" and link.from_node.image:
                    img = link.from_node.image
                    return img.size[0], img.size[1], img.pixels[:]
    return None


def prop_flags(mat):
    """Team color, glow and gloss come from material names, so an artist
    marks them in Blender: "Team...", "Glow...", "Gloss..."."""
    if mat is None:
        return 0
    name = mat.name.lower()
    flags = 1 if name.startswith("team") or mat.name in TEAM_MATERIALS else 0
    flags |= 2 if name.startswith("glow") else 0
    flags |= 4 if name.startswith("gloss") else 0
    # What the surface is made of, for the game's shader to paint (MAT_* in
    # crates/client/src/model.rs): a word anywhere in the name.
    for i, words in enumerate(MATERIAL_WORDS, start=1):
        if any(w in name for w in words):
            flags |= i << 3
            break
    return flags


class PropBaker(Baker):
    """One frame of a static model. Colors come per face corner from the
    texture (palette-textured asset packs), else the vertex colors, else the
    material, so a flat-shaded look survives without texture support."""

    def capture(self):
        self.keys = []
        vert_index = {}
        frame = []
        for obj in self.objects:
            ev, mesh = self._evaluated(obj)
            mw = ev.matrix_world
            nm = mw.to_3x3().inverted_safe().transposed()
            mats = [s.material for s in obj.material_slots]
            images = {}
            uv = mesh.uv_layers.active
            cols = mesh.color_attributes.active_color
            corner_normals = [cn.vector.copy() for cn in mesh.corner_normals]
            for tri in mesh.loop_triangles:
                mat = mats[tri.material_index] if tri.material_index < len(mats) else None
                if tri.material_index not in images:
                    images[tri.material_index] = base_image(mat)
                img = images[tri.material_index]
                flags = prop_flags(mat)
                for li in tri.loops:
                    vi = mesh.loops[li].vertex_index
                    if img and uv:
                        w, h, px = img
                        u, v = uv.data[li].uv
                        x = min(w - 1, max(0, int((u % 1.0) * w)))
                        y = min(h - 1, max(0, int((v % 1.0) * h)))
                        rgb = px[(y * w + x) * 4:(y * w + x) * 4 + 3]
                    elif cols is not None:
                        c = cols.data[li if cols.domain == "CORNER" else vi].color
                        rgb = c[:3]
                    else:
                        rgb, _ = material_color(mat)
                    color = (*[round(linear_to_srgb(c) * 255) for c in rgb], flags)
                    n = (nm @ corner_normals[li]).normalized()
                    p = ((mw @ mesh.vertices[vi].co) - self.origin) * self.scale
                    key = (color, tuple(round(c, 5) for c in p), tuple(round(c, 3) for c in n))
                    if key not in vert_index:
                        vert_index[key] = len(self.colors)
                        self.colors.append(color)
                        frame.append((p, n))
                    self.indices.append(vert_index[key])
            ev.to_mesh_clear()
        self.frames = [frame]


def export_prop():
    """prop SOURCE NAME [--size TILES] [--turn DEGREES] [--decimate RATIO]

    Scales the model so its widest side is --size tiles (default: as
    modelled, one Blender unit per tile), turns it about z, sits it on the
    ground centred on the origin, and writes assets/models/NAME.sbm. The
    game faces models toward -y with z up."""
    args = sys.argv[sys.argv.index("--") + 1:]
    src, name = args[1], args[2]
    load_source(src)
    show_everything()
    objs = [o for o in bpy.context.scene.objects if o.type == "MESH" and not o.hide_render]
    if not objs:
        sys.exit(f"{src} has no meshes")
    turn = Matrix.Rotation(math.radians(_arg("--turn", 0.0)), 4, "Z")
    for o in objs:
        o.matrix_world = turn @ o.matrix_world
        if "--decimate" in sys.argv:
            d = o.modifiers.new("lite", "DECIMATE")
            d.ratio = _arg("--decimate", 1.0)
    bpy.context.view_layer.update()
    corners = [o.matrix_world @ Vector(c) for o in objs for c in o.bound_box]
    lo = Vector([min(c[i] for c in corners) for i in range(3)])
    hi = Vector([max(c[i] for c in corners) for i in range(3)])
    widest = max(hi.x - lo.x, hi.y - lo.y)
    scale = _arg("--size", widest) / widest if widest > 0 else 1.0
    origin = Vector(((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, lo.z))
    baker = PropBaker(objs, origin, scale)
    baker.capture()
    write_model(f"{OUT_DIR}/{name}.sbm", [baker.encode(name)])


# --------------------------------------------------------------------------
# Units made by tools/penguins.py (or by hand in the same layout): every
# mesh in the scene is baked, and each timeline marker starts a clip that
# runs up to the next marker. Material names starting "Team", "Glow" or
# "Gloss" set those flags.
#
#     blender -b art/units/skier.blend --python tools/export_models.py -- unit skier


def export_unit(name=None):
    scene = bpy.context.scene
    if name is None:
        name = sys.argv[sys.argv.index("--") + 2]
    objs = sorted((o for o in scene.objects if o.type == "MESH" and not o.hide_render), key=lambda o: o.name)
    markers = sorted(scene.timeline_markers, key=lambda m: m.frame)
    clips = [(m.name, m.frame, markers[i + 1].frame) for i, m in enumerate(markers) if i + 1 < len(markers)]
    encoded = []
    for clip, start, end in clips:
        if clip == "end":
            continue
        baker = Baker(objs, Vector((0, 0, 0)), 1.0)
        for f in range(start, end):
            scene.frame_set(f)
            bpy.context.view_layer.update()
            baker.capture()
        encoded.append(baker.encode(clip))
    write_model(f"{OUT_DIR}/unit-{name}.sbm", encoded)


if __name__ == "__main__":
    what = sys.argv[sys.argv.index("--") + 1]
    {"penguin": export_penguin, "tree": export_tree, "prop": export_prop, "unit": export_unit}[what]()
