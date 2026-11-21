"""Buildings made in Blender, in place of the plain built-in (meshgen) ones.

    blender -b --python tools/buildings.py                  # every building
    blender -b --python tools/buildings.py -- tower palisade-0    # just these
    blender -b --python tools/buildings.py -- tower --preview   # and a picture

Each is modelled here from code, so it can be tuned and rebuilt, and writes
art/buildings/<name>.blend (an editable copy) and assets/models/<name>.sbm
(what the game draws; see docs/ART.md). --preview also renders
/tmp/snowballz-<name>.png to look at.

Game units: a tile is 1, z is up, the front faces -Y, and the model stands
on z = 0. Material names say what each part is made of ("Snow...",
"Wood...", "Stone...", "Ice...", "Metal..."; "Team..." takes the owner's
colour, "Glow..." stays lit, "Gloss..." shines), and the game's shader
paints the surface to match.
"""

import math
import os
import random
import sys

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import export_models as em  # noqa: E402

ART = "art/buildings"
TAU = math.tau


def srgb_to_linear(c):
    c /= 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def mat(name, rgb):
    m = bpy.data.materials.get(name)
    if m is None:
        m = bpy.data.materials.new(name)
        m.use_nodes = True
        bsdf = m.node_tree.nodes["Principled BSDF"]
        lin = [srgb_to_linear(c) for c in rgb]
        bsdf.inputs["Base Color"].default_value = (*lin, 1.0)
        m.diffuse_color = (*lin, 1.0)
    return m


def _object(name, mesh, material, smooth):
    mesh.materials.append(material)
    for p in mesh.polygons:
        p.use_smooth = smooth
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def box(lo, hi, material, bevel=0.0, segments=1, smooth=False, name="box"):
    """An axis-aligned block from corner `lo` to `hi`, its edges bevelled."""
    lo, hi = Vector(lo), Vector(hi)
    mesh = bpy.data.meshes.new(name)
    v = [(x, y, z) for z in (lo.z, hi.z) for y in (lo.y, hi.y) for x in (lo.x, hi.x)]
    f = [(0, 2, 3, 1), (4, 5, 7, 6), (0, 1, 5, 4), (2, 6, 7, 3), (0, 4, 6, 2), (1, 3, 7, 5)]
    mesh.from_pydata(v, [], f)
    obj = _object(name, mesh, material, smooth)
    if bevel > 0:
        size = hi - lo
        b = obj.modifiers.new("bevel", "BEVEL")
        b.width = min(bevel, min(size) * 0.45)
        b.segments = segments
        b.limit_method = "NONE"
    return obj


def pillow(lo, hi, material, round_=0.05):
    """A soft, rounded lump: snow lying on top of something."""
    return box(lo, hi, material, bevel=round_, segments=2, smooth=True, name="snow")


def cone(base, axis, radius, length, material, sides=6, smooth=False, name="cone"):
    axis = Vector(axis).normalized()
    t1 = axis.cross(Vector((0, 0, 1)) if abs(axis.z) < 0.9 else Vector((1, 0, 0))).normalized()
    t2 = axis.cross(t1)
    base = Vector(base)
    v = [base + (t1 * math.cos(a) + t2 * math.sin(a)) * radius for a in (i / sides * TAU for i in range(sides))]
    v.append(base + axis * length)
    f = [(i, (i + 1) % sides, sides) for i in range(sides)] + [tuple(reversed(range(sides)))]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata([tuple(p) for p in v], [], f)
    return _object(name, mesh, material, smooth)


def prism(base, axis, radius, length, material, sides=6, smooth=True, name="pole"):
    axis = Vector(axis).normalized()
    t1 = axis.cross(Vector((0, 0, 1)) if abs(axis.z) < 0.9 else Vector((1, 0, 0))).normalized()
    t2 = axis.cross(t1)
    base = Vector(base)
    ring = [(t1 * math.cos(a) + t2 * math.sin(a)) * radius for a in (i / sides * TAU for i in range(sides))]
    v = [base + r for r in ring] + [base + axis * length + r for r in ring]
    f = [(i, (i + 1) % sides, sides + (i + 1) % sides, sides + i) for i in range(sides)]
    f += [tuple(reversed(range(sides))), tuple(range(sides, 2 * sides))]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata([tuple(p) for p in v], [], f)
    return _object(name, mesh, material, smooth)


def poly(points, material, name="poly"):
    """A flat polygon, seen from both sides (flags, banners)."""
    mesh = bpy.data.meshes.new(name)
    n = len(points)
    mesh.from_pydata(points, [], [tuple(range(n)), tuple(reversed(range(n)))])
    return _object(name, mesh, material, False)


def obox(center, size, rot, material, bevel=0.0, smooth=False, segments=1, name="box"):
    """A block of `size` centred at `center`, turned by Euler angles `rot`."""
    h = Vector(size) / 2
    obj = box(-h, h, material, bevel=bevel, segments=segments, smooth=smooth, name=name)
    obj.location = center
    obj.rotation_euler = rot
    return obj


def log(a, b, radius, material, sides=7):
    """A round log from point `a` to point `b`."""
    a, b = Vector(a), Vector(b)
    return prism(a, b - a, radius, (b - a).length, material, sides=sides, smooth=True, name="log")


def ball(center, radius, material, subdivisions=2, scale=(1, 1, 1), name="ball"):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=subdivisions, radius=radius, location=center)
    obj = bpy.context.object
    obj.name = name
    obj.scale = scale
    obj.data.materials.append(material)
    for p in obj.data.polygons:
        p.use_smooth = True
    return obj


def stone(rng, P, center, size):
    s = (size * rng.uniform(0.9, 1.2), size * rng.uniform(0.8, 1.1), size * rng.uniform(0.6, 0.8))
    ball(center, 1.0, P["stone"], subdivisions=1, scale=s, name="stone")


def fire(rng, P, at, size=1.0):
    """Crossed logs, flames in two colours and a ring of stones."""
    at = Vector(at)
    for k in range(8):
        a = k / 8 * TAU
        stone(rng, P, at + Vector((math.cos(a) * 0.27, math.sin(a) * 0.27, 0.03)) * size, 0.075 * size)
    for k in range(4):
        a = k / 4 * TAU + 0.3
        foot = at + Vector((math.cos(a) * 0.2, math.sin(a) * 0.2, 0.03)) * size
        log(foot, at + Vector((0, 0, 0.26 * size)), 0.035 * size, P["dark"], sides=6)
    cone(at + Vector((0, 0, 0.04)), (0, 0, 1), 0.13 * size, 0.4 * size, P["flame"], sides=7, smooth=True)
    cone(at + Vector((0.02, -0.01, 0.04)), (0.05, 0, 1), 0.075 * size, 0.3 * size, P["flame2"], sides=6, smooth=True)
    for k in range(3):
        a = k / 3 * TAU + 1.0
        cone(at + Vector((math.cos(a) * 0.08, math.sin(a) * 0.08, 0.04)) * 1.0, (math.cos(a) * 0.3, math.sin(a) * 0.3, 1), 0.05 * size, 0.2 * size, P["flame"], sides=5, smooth=True)


def fish(P, at, hang=True):
    """A fish hanging by its tail (or lying flat)."""
    at = Vector(at)
    ball(at, 1.0, P["fish"], subdivisions=1, scale=(0.035, 0.022, 0.11) if hang else (0.11, 0.035, 0.022), name="fish")
    if hang:
        t = at + Vector((0, 0, 0.1))
        poly([tuple(t), tuple(t + Vector((-0.045, 0, 0.06))), tuple(t + Vector((0.045, 0, 0.06)))], P["fish"], "tail")


def triangles():
    deps = bpy.context.evaluated_depsgraph_get()
    total = 0
    for o in bpy.context.scene.objects:
        if o.type == "MESH":
            m = o.evaluated_get(deps).to_mesh()
            m.calc_loop_triangles()
            total += len(m.loop_triangles)
            o.evaluated_get(deps).to_mesh_clear()
    return total


def finish(name, stages=True):
    """Save the editable copy and bake the game model. Igloos are never
    built, so they have no construction stages."""
    os.makedirs(ART, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(f"{ART}/{name}.blend"))
    objs = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    bpy.context.view_layer.update()
    corners = [o.matrix_world @ Vector(c) for o in objs for c in o.bound_box]
    lo = Vector([min(c[i] for c in corners) for i in range(3)])
    hi = Vector([max(c[i] for c in corners) for i in range(3)])
    # Centred on the footprint the game gives it, not on whatever sticks out.
    origin = Vector((0.0, 0.0, lo.z))
    baker = em.PropBaker(objs, origin, 1.0)
    baker.capture()
    anims = [baker.encode(name)]
    for k, cut in enumerate(STAGES if stages else (), start=1):
        anims.append(stage(objs, origin, lo, hi, k, cut))
    em.write_model(f"{em.OUT_DIR}/{name}.sbm", anims)
    print(f"{name}: {triangles()} triangles, {len(baker.colors)} vertices, {hi.x - lo.x:.2f} x {hi.y - lo.y:.2f} x {hi.z - lo.z:.2f}")
    if "--preview" in sys.argv:
        preview(name)


# Construction stages: a site shows the parts that start below this share
# of the building's height (it goes up course by course), in scaffolding.
STAGES = (0.12, 0.42, 0.72)


def stage(objs, origin, lo, hi, k, cut):
    """The building part-built, as the game model's "stage-K" animation."""
    P = palette()
    top = lo.z + (hi.z - lo.z) * cut
    # Filler cores and dark insides only go in at the end: until then the
    # site is an open ring of blocks.
    keep = [o for o in objs if not o.get("finished_only") and not o.name.startswith(("core", "inside")) and min((o.matrix_world @ Vector(c)).z for c in o.bound_box) <= top]
    extra = []
    big = min(hi.x - lo.x, hi.y - lo.y) > 1.2
    rng = random.Random(k)
    if big:
        # Scaffold poles at the corners, lashed rails at the working height.
        x0, x1, y0, y1 = lo.x - 0.06, hi.x + 0.06, lo.y - 0.06, hi.y + 0.06
        x0, x1 = max(x0, -1.05), min(x1, 1.05)
        y0, y1 = max(y0, -1.05), min(y1, 1.05)
        h = top - lo.z + 0.35
        for x, y in ((x0, y0), (x1, y0), (x1, y1), (x0, y1)):
            extra.append(log((x, y, 0), (x + rng.uniform(-0.03, 0.03), y + rng.uniform(-0.03, 0.03), h), 0.03, P["wood"], sides=5))
        if k > 1:
            z = h - 0.3
            for a, b in (((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))):
                extra.append(log((*a, z), (*b, z), 0.02, P["dark"], sides=5))
            # A plank walkway along the front.
            extra.append(box((x0, y0 - 0.12, z + 0.02), (x1, y0 + 0.04, z + 0.05), P["wood"]))
    # Snow blocks waiting to go up, and a couple of planks.
    bx, by = (min(hi.x + 0.05, 0.75), max(lo.y - 0.05, -0.75)) if big else (0.3, -0.38)
    for i in range(3 if big else 2):
        z = (i // 2) * 0.13
        extra.append(box((bx - 0.2 + (i % 2) * 0.2, by - 0.1, z), (bx - 0.02 + (i % 2) * 0.2, by + 0.08, z + 0.12), P["snow"], bevel=0.02))
    if big:
        for i in range(2):
            extra.append(box((lo.x + 0.1, lo.y - 0.25 + i * 0.07, i * 0.03), (lo.x + 0.9, lo.y - 0.2 + i * 0.07, i * 0.03 + 0.03), P["wood"]))
    bpy.context.view_layer.update()
    baker = em.PropBaker(keep + extra, origin, 1.0)
    baker.capture()
    for o in extra:
        bpy.data.objects.remove(o)
    return baker.encode(f"stage-{k}")


def preview(name):
    scene = bpy.context.scene
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    scene.collection.objects.link(cam)
    # Back off to fit the model: a 2-tile building at 1.
    corners = [o.matrix_world @ Vector(c) for o in scene.objects if o.type == "MESH" for c in o.bound_box]
    span = max(max(c[i] for c in corners) - min(c[i] for c in corners) for i in range(3))
    far = max(span / 2.6, 0.35)
    cam.location = (-3.2 * far, -4.6 * far, 3.6 * far)
    cam.rotation_euler = (math.radians(58), 0, math.radians(-35))
    cam.data.lens = 40
    scene.camera = cam
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    scene.collection.objects.link(sun)
    sun.rotation_euler = (math.radians(50), math.radians(10), math.radians(-40))
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "MATERIAL"
    scene.display.shading.show_shadows = True
    scene.display.shading.show_cavity = True
    scene.render.resolution_x, scene.render.resolution_y = 900, 900
    scene.render.filepath = f"/tmp/snowballz-{name}.png"
    bpy.ops.render.render(write_still=True)


# --------------------------------------------------------------------------
# Shared parts


def palette():
    return {
        "snow": mat("Snow Block", (228, 236, 248)),
        "cap": mat("Snow Cap", (250, 252, 255)),
        "shade": mat("Snow Shade", (196, 210, 232)),
        "wood": mat("Wood", (132, 92, 56)),
        "dark": mat("Wood Dark", (86, 60, 40)),
        "hole": mat("Shadow", (30, 34, 46)),
        "ice": mat("Gloss Ice", (176, 220, 246)),
        "team": mat("Team Cloth", (255, 255, 255)),
        "lamp": mat("Glow Lamp", (255, 196, 110)),
        "iron": mat("Metal Iron", (58, 60, 66)),
        "stone": mat("Stone", (120, 124, 132)),
        "flame": mat("Glow Flame", (255, 140, 40)),
        "flame2": mat("Glow Flame Core", (255, 226, 120)),
        "fish": mat("Gloss Fish", (150, 170, 190)),
        "rope": mat("Rope Cloth", (190, 165, 120)),
    }


def block_course(rng, P, e, z0, z1, depth, length, own_x, stagger):
    """One course of snow blocks round a square, `e` out from the centre.
    The pair of sides that owns the corners swaps each course, like brickwork."""
    gap = 0.014
    for side in range(4):
        along_x = side % 2 == 0
        owns = along_x == own_x
        lo_t = -e if owns else -e + depth + gap
        hi_t = e if owns else e - depth - gap
        t = lo_t - (length * 0.5 if stagger else 0.0)
        while t < hi_t - 0.05:
            a, b = max(t, lo_t) + gap, min(t + length, hi_t) - gap
            t += length
            if b - a < 0.08:
                continue
            h0, h1 = z0 + gap, z1 - gap + rng.uniform(-0.01, 0.0)
            out = rng.uniform(-0.008, 0.008)
            sgn = -1 if side < 2 else 1
            if along_x:
                y0, y1 = (sgn * e - depth, sgn * e + out) if sgn > 0 else (sgn * e - out, sgn * e + depth)
                box((a, y0, h0), (b, y1, h1), P["snow"], bevel=0.03)
            else:
                x0, x1 = (sgn * e - depth, sgn * e + out) if sgn > 0 else (sgn * e - out, sgn * e + depth)
                box((x0, a, h0), (x1, b, h1), P["snow"], bevel=0.03)


def icicles(rng, P, points):
    for p in points:
        cone(p, (0, 0, -1), rng.uniform(0.018, 0.03), rng.uniform(0.07, 0.2), P["ice"], sides=5)


def pennant(P, base, h, toward=(1, 0)):
    """The owner's pennant: it goes up when the building is finished."""
    top = Vector(base) + Vector((0, 0, h - 0.02))
    d = Vector((toward[0], toward[1], 0)) * 0.42
    parts = [
        prism(base, (0, 0, 1), 0.025, h, P["dark"], sides=6),
        poly([tuple(top), tuple(top + d + Vector((0, 0, -0.12))), tuple(top + Vector((0, 0, -0.26)))], P["team"], "pennant"),
        cone(Vector(base) + Vector((0, 0, h)), (0, 0, 1), 0.04, 0.06, P["iron"], sides=6),
    ]
    for o in parts:
        o["finished_only"] = True


# --------------------------------------------------------------------------
# Buildings


def yard(rng, P, fence_sides="back", crates=2, barrel=True, tracks=True):
    """The ground of a camp: trodden snow across the plot, a rope fence on
    stakes along the back (and sides), crates and a barrel by the fence,
    footprints. Fills the 2x2 plot so a camp reads as a busy yard."""
    trod = mat("Snow Trodden", (206, 216, 232))
    poly([(-1.0, -1.0, 0.012), (1.0, -1.0, 0.012), (1.0, 1.0, 0.012), (-1.0, 1.0, 0.012)], trod, "ground")
    for _ in range(5):
        a, d = rng.uniform(0, TAU), rng.uniform(0.85, 1.0)
        ball((math.cos(a) * d, math.sin(a) * d, 0), 1.0, P["cap"], subdivisions=1, scale=(rng.uniform(0.15, 0.3), rng.uniform(0.15, 0.3), 0.06), name="snow")
    sides = {"none": [], "back": [((-0.97, 0.97), (0.97, 0.97))], "u": [((-0.97, 0.97), (0.97, 0.97)), ((-0.97, -0.6), (-0.97, 0.97)), ((0.97, -0.6), (0.97, 0.97))]}[fence_sides]
    for a, b in sides:
        a, b = Vector((*a, 0)), Vector((*b, 0))
        n = max(2, round((b - a).length / 0.48))
        for k in range(n + 1):
            p = a + (b - a) * (k / n)
            log(p, p + Vector((rng.uniform(-0.02, 0.02), rng.uniform(-0.02, 0.02), 0.34)), 0.022, P["dark"], sides=5)
            ball(tuple(p + Vector((0, 0, 0.35))), 0.03, P["cap"], subdivisions=1)
        for z in (0.16, 0.29):
            log(a + Vector((0, 0, z)), b + Vector((0, 0, z - 0.02)), 0.009, P["rope"], sides=4)
    spots = [(-0.82, 0.72), (0.8, 0.74), (-0.84, -0.8)]
    for i in range(crates):
        x, y = spots[i]
        box((x - 0.11, y - 0.11, 0), (x + 0.11, y + 0.11, 0.2), P["wood"], bevel=0.012)
        box((x - 0.115, y - 0.02, 0.06), (x + 0.115, y + 0.02, 0.14), P["dark"])
        pillow((x - 0.1, y - 0.1, 0.18), (x + 0.1, y + 0.1, 0.23), P["cap"], round_=0.02)
    if barrel:
        prism((0.82, -0.82, 0), (0, 0, 1), 0.1, 0.24, P["wood"], sides=10)
        for z in (0.05, 0.18):
            prism((0.82, -0.82, z), (0, 0, 1), 0.105, 0.02, P["iron"], sides=10)
        ball((0.82, -0.82, 0.25), 1.0, P["cap"], subdivisions=1, scale=(0.09, 0.09, 0.03))
    if tracks:
        foot = mat("Snow Print", (186, 198, 222))
        for k in range(7):
            x, y = -0.2 + k * 0.03 * (1 if k % 2 else -1), -0.98 + k * 0.13
            obox((x + (0.05 if k % 2 else -0.05), y, 0.016), (0.06, 0.09, 0.004), (0, 0, 0.1), foot)



def tower():
    """Snowball tower (2x2): stacked snow blocks, a plank fighting deck
    with snow-capped merlons, a plank door with a lamp, the owner's banner."""
    rng = random.Random(41)
    P = palette()
    yard(rng, P, fence_sides="none", crates=0, barrel=False)
    hw, deck = 0.86, 1.6
    box((-hw + 0.1, -hw + 0.1, 0), (hw - 0.1, hw - 0.1, deck), P["shade"], name="core")
    courses = 5
    ch = deck / courses
    for k in range(courses):
        e = hw - 0.035 * k / courses
        block_course(rng, P, e, k * ch, (k + 1) * ch, 0.2, 0.62, k % 2 == 0, k % 2 == 1)
    # Snow drifted against the foot.
    pillow((-hw - 0.16, -hw - 0.16, 0), (hw + 0.16, hw + 0.16, 0.2), P["cap"], round_=0.15)
    # Plank deck overhanging the walls, on beams.
    o = hw + 0.1
    for i in range(7):
        x0 = -o + i * (2 * o / 7)
        box((x0 + 0.008, -o, deck), (x0 + 2 * o / 7 - 0.008, o, deck + 0.07), P["wood"], bevel=0.012)
    for side in range(4):
        a = side * TAU / 4
        c, s = math.cos(a), math.sin(a)
        for m in (-0.5, 0.0, 0.5):
            cx, cy = c * (hw + 0.04) - s * m, s * (hw + 0.04) + c * m
            ex, ey = abs(c) * 0.09 + abs(s) * 0.05, abs(s) * 0.09 + abs(c) * 0.05
            box((cx - ex, cy - ey, deck - 0.1), (cx + ex, cy + ey, deck), P["dark"])
    # Icicles along the deck's edge.
    pts = []
    for side in range(4):
        a = side * TAU / 4
        c, s = math.cos(a), math.sin(a)
        for _ in range(6):
            m = rng.uniform(-o + 0.08, o - 0.08)
            pts.append((c * (o - 0.03) - s * m, s * (o - 0.03) + c * m, deck))
    icicles(rng, P, pts)
    # Parapet: a low wall all round with merlons on the corners and the
    # middle of each side, each under a cap of snow.
    top = deck + 0.07
    pe = hw - 0.02
    box((-pe, -pe, top), (pe, -pe + 0.16, top + 0.14), P["snow"], bevel=0.025)
    box((-pe, pe - 0.16, top), (pe, pe, top + 0.14), P["snow"], bevel=0.025)
    box((-pe, -pe + 0.17, top), (-pe + 0.16, pe - 0.17, top + 0.14), P["snow"], bevel=0.025)
    box((pe - 0.16, -pe + 0.17, top), (pe, pe - 0.17, top + 0.14), P["snow"], bevel=0.025)
    for mx in (-1, 0, 1):
        for my in (-1, 0, 1):
            if mx == 0 and my == 0:
                continue
            cx, cy = mx * (pe - 0.17), my * (pe - 0.17)
            h = top + 0.38 + rng.uniform(-0.02, 0.02)
            box((cx - 0.17, cy - 0.17, top), (cx + 0.17, cy + 0.17, h), P["snow"], bevel=0.03)
            pillow((cx - 0.19, cy - 0.19, h - 0.03), (cx + 0.19, cy + 0.19, h + 0.07), P["cap"], round_=0.05)
    # A stack of snowballs ready on the deck.
    for i, (x, y, z) in enumerate([(-0.15, 0.1, 0), (0.08, 0.1, 0), (-0.04, -0.08, 0), (-0.04, 0.03, 0.15)]):
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1, radius=0.09, location=(x, y, top + 0.09 + z))
        ball = bpy.context.object
        ball.data.materials.append(P["cap"])
        for p in ball.data.polygons:
            p.use_smooth = True
    # The door: a dark opening, a plank door standing in it, a frame and a lamp.
    f = -hw - 0.01
    box((-0.25, f - 0.02, 0.0), (0.25, f + 0.12, 0.66), P["hole"])
    for i in range(3):
        x0 = -0.2 + i * 0.4 / 3
        box((x0 + 0.006, f - 0.05, 0.02), (x0 + 0.4 / 3 - 0.006, f + 0.0, 0.6), P["wood"], bevel=0.01)
    box((-0.18, f - 0.07, 0.38), (0.18, f - 0.04, 0.44), P["dark"], bevel=0.008)
    box((-0.18, f - 0.07, 0.12), (0.18, f - 0.04, 0.18), P["dark"], bevel=0.008)
    for x in (-0.27, 0.27):
        box((x - 0.04, f - 0.08, 0), (x + 0.04, f + 0.02, 0.7), P["dark"], bevel=0.012)
    box((-0.33, f - 0.09, 0.66), (0.33, f + 0.02, 0.75), P["dark"], bevel=0.012)
    pillow((-0.35, f - 0.11, 0.74), (0.35, f + 0.04, 0.8), P["cap"], round_=0.03)
    box((0.38, f - 0.12, 0.78), (0.41, f, 0.81), P["iron"])
    box((0.36, f - 0.16, 0.66), (0.43, f - 0.09, 0.78), P["lamp"], bevel=0.01)
    # Arrow slits on every side.
    for side in range(4):
        a = side * TAU / 4 + TAU / 4 * 3
        c, s = math.cos(a), math.sin(a)
        for m in ((-0.42, 0.42) if side != 0 else (-0.5, 0.5)):
            cx, cy = c * (hw + 0.005) - s * m, s * (hw + 0.005) + c * m
            ex, ey = abs(s) * 0.035 + abs(c) * 0.03, abs(c) * 0.035 + abs(s) * 0.03
            box((cx - ex, cy - ey, 1.0), (cx + ex, cy + ey, 1.32), P["hole"])
    # The owner's banner hanging from the deck on the front.
    box((0.3, f - 0.08, deck - 0.14), (0.78, f - 0.04, deck - 0.1), P["dark"])
    poly([(0.34, f - 0.065, deck - 0.12), (0.74, f - 0.065, deck - 0.12), (0.74, f - 0.065, deck - 0.78), (0.54, f - 0.065, deck - 0.66), (0.34, f - 0.065, deck - 0.78)], P["team"], "banner")["finished_only"] = True
    pennant(P, (pe - 0.17, pe - 0.17, top + 0.45), 0.8)
    finish("tower")


def wall_courses(rng, P, depth, heights, taper):
    """Courses of blocks one tile long (x -0.5..0.5), right through the wall,
    staggered, so neighbouring pieces join into one wall."""
    z = 0.0
    for k, h in enumerate(heights):
        d = depth - taper * k
        t = -0.5 - (0.25 if k % 2 else 0.0)
        while t < 0.5 - 0.02:
            a, b = max(t, -0.5) + 0.012, min(t + 0.5, 0.5) - 0.012
            t += 0.5
            if b - a < 0.06:
                continue
            dy = rng.uniform(-0.012, 0.012)
            box((a, -d + dy, z + 0.01), (b, d + dy, z + h - 0.01 + rng.uniform(-0.012, 0.0)), P["snow"], bevel=0.028)
        z += h
    box((-0.5, -depth + taper * len(heights) + 0.03, 0), (0.5, depth - taper * len(heights) - 0.03, z - 0.02), P["shade"], name="core")
    return z


def snow_lumps(rng, P, z, depth, n):
    """Uneven snow lying along the top of a wall piece."""
    edges = [-0.5] + sorted(rng.uniform(-0.3, 0.3) for _ in range(n - 1)) + [0.5]
    for a, b in zip(edges, edges[1:]):
        h = rng.uniform(0.05, 0.1)
        pillow((a, -depth - 0.02, z - 0.04), (b, depth + 0.02, z + h), P["cap"], round_=0.045)


def palisade(seed):
    """Snow palisade piece (one tile): three courses of big blocks, leaning
    in, a toothed top under snow, bound by a timber through the middle."""
    rng = random.Random(seed * 47 + 11)
    P = palette()
    depth = 0.4
    z = wall_courses(rng, P, depth, [0.4, 0.38, 0.34], 0.035)
    top = depth - 0.035 * 3
    for x in (-0.25, 0.25):
        h = 0.2 + rng.uniform(-0.02, 0.02)
        box((x - 0.19, -top, z), (x + 0.19, top, z + h), P["snow"], bevel=0.028)
        pillow((x - 0.21, -top - 0.02, z + h - 0.03), (x + 0.21, top + 0.02, z + h + 0.06), P["cap"], round_=0.04)
    pillow((-0.5, -top - 0.01, z - 0.03), (0.5, top + 0.01, z + 0.05), P["cap"], round_=0.03)
    # A timber tie across both faces, pegged.
    for sgn in (-1, 1):
        y = sgn * (depth - 0.035)
        box((-0.5, y - 0.04, 0.5), (0.5, y + 0.04, 0.6), P["wood"], bevel=0.012)
        box((-0.03, y + sgn * 0.03, 0.48), (0.03, y + sgn * 0.07, 0.62), P["dark"], bevel=0.01)
    pillow((-0.5, -depth - 0.14, 0), (0.5, depth + 0.14, 0.14), P["cap"], round_=0.1)
    if seed == 0:
        icicles(rng, P, [(rng.uniform(-0.4, 0.4), -depth + 0.02, 0.5) for _ in range(4)])
    finish(f"palisade-{seed}")


def sauna():
    """Sauna (2x2): a low snow-block dome open at the front, a fire burning
    before the door, a stack of firewood and the owner's pennant."""
    rng = random.Random(11)
    P = palette()
    yard(rng, P, crates=1, barrel=False)
    r, h = 0.82, 0.8
    rows = 4
    for row in range(rows):
        el = (row + 0.5) / rows * math.pi / 2
        cols = max(4, round(math.cos(el) * 11))
        for c in range(cols):
            az = (c + 0.5 * (row % 2)) / cols * TAU
            if row < 2 and abs(((az - 1.5 * math.pi + math.pi) % TAU) - math.pi) < 0.6:
                continue
            centre = Vector((math.cos(el) * math.cos(az) * r, math.cos(el) * math.sin(az) * r, math.sin(el) * h))
            length = TAU * math.cos(el) * r / cols - 0.03
            tangent = Vector((-math.sin(az), math.cos(az), 0))
            outward = Vector((math.cos(el) * math.cos(az) / r, math.cos(el) * math.sin(az) / r, math.sin(el) / h)).normalized()
            up = outward.cross(tangent)
            blk = obox((0, 0, 0), (length, math.pi / 2 / rows * h * 1.2 - 0.03, 0.15), (0, 0, 0), P["snow"], bevel=0.025)
            blk.matrix_world = Matrix((
                (tangent.x, up.x, outward.x, centre.x),
                (tangent.y, up.y, outward.y, centre.y),
                (tangent.z, up.z, outward.z, centre.z),
                (0, 0, 0, 1),
            ))
    ball((0, 0, 0), 0.97, P["hole"], subdivisions=2, scale=(r * 0.92, r * 0.92, h * 0.92), name="inside")
    pillow((-0.1, -0.1, h - 0.06), (0.1, 0.1, h + 0.05), P["cap"], round_=0.04)
    # Door frame of two posts and a lintel.
    for x in (-0.3, 0.3):
        log((x, -r + 0.02, 0), (x, -r + 0.02, 0.52), 0.035, P["dark"])
    log((-0.36, -r + 0.02, 0.52), (0.36, -r + 0.02, 0.52), 0.035, P["dark"])
    pillow((-0.38, -r - 0.03, 0.54), (0.38, -r + 0.07, 0.6), P["cap"], round_=0.025)
    fire(rng, P, (0, -r - 0.3, 0), 0.9)
    # Firewood stacked by the door.
    for k in range(6):
        z = 0.045 + (k // 3) * 0.085
        x = 0.5 + (k % 3) * 0.09 + (k // 3) * 0.045
        log((x, -0.72, z), (x + rng.uniform(-0.02, 0.02), -1.02, z), 0.042, P["wood"])
    pillow((0.45, -1.04, 0.2), (0.78, -0.7, 0.24), P["cap"], round_=0.02)
    pennant(P, (-0.72, -0.62, 0), 1.25)
    finish("sauna")


def campfire():
    """Campfire (1 tile): a ring of stones, crossed logs, flames and a log to sit on."""
    rng = random.Random(23)
    P = palette()
    fire(rng, P, (0, 0, 0), 1.1)
    log((-0.32, 0.38, 0.06), (0.3, 0.42, 0.06), 0.06, P["wood"])
    pillow((-0.25, 0.34, 0.1), (0.15, 0.46, 0.14), P["cap"], round_=0.02)
    finish("campfire")


def store():
    """Fish store (2x2): a log cabin under a snowy roof, with a door, a
    barrel and crates, and fish drying on a rack out front."""
    rng = random.Random(23)
    P = palette()
    yard(rng, P, crates=0, barrel=False)
    hw, hd, wall_h = 0.6, 0.45, 0.6
    cy = 0.15
    n = 6
    for k in range(n):
        z = 0.05 + k * (wall_h - 0.05) / n
        r = 0.05
        for y in (cy - hd, cy + hd):
            log((-hw - 0.08, y, z + (0.0 if k % 2 else r)), (hw + 0.08, y, z + (0.0 if k % 2 else r)), r, P["wood"])
        for x in (-hw, hw):
            log((x, cy - hd - 0.08, z + (r if k % 2 else 0.0)), (x, cy + hd + 0.08, z + (r if k % 2 else 0.0)), r, P["wood"])
    box((-hw + 0.03, cy - hd + 0.03, 0), (hw - 0.03, cy + hd - 0.03, wall_h), P["dark"], name="core")
    # Door.
    f = cy - hd - 0.06
    for i in range(3):
        x0 = -0.16 + i * 0.32 / 3
        box((x0 + 0.005, f - 0.02, 0.02), (x0 + 0.32 / 3 - 0.005, f + 0.03, 0.46), P["wood"], bevel=0.008)
    box((-0.19, f - 0.04, 0.46), (0.19, f + 0.03, 0.52), P["dark"], bevel=0.008)
    # Pitched plank roof, then snow lying thick on it.
    ridge = wall_h + 0.42
    ox, oy = hw + 0.14, hd + 0.16
    pitch = math.atan2(ridge - wall_h, oy)
    slope_len = math.hypot(oy, ridge - wall_h) + 0.02
    for sgn in (-1, 1):
        mid = Vector((0, cy + sgn * oy / 2, (wall_h + ridge) / 2 + 0.02))
        obox(mid, (2 * ox, slope_len, 0.05), (-sgn * pitch, 0, 0), P["dark"], bevel=0.01)
        obox(mid + Vector((0, sgn * 0.01, 0.06)), (2 * ox - 0.06, slope_len - 0.08, 0.08), (-sgn * pitch, 0, 0), P["cap"], bevel=0.035, segments=2, smooth=True)
    log((-ox - 0.04, cy, ridge + 0.02), (ox + 0.04, cy, ridge + 0.02), 0.045, P["wood"])
    icicles(rng, P, [(rng.uniform(-ox, ox), cy - oy + 0.02, wall_h - 0.02) for _ in range(6)])
    # A barrel and crates by the wall.
    prism((hw + 0.05, cy - hd - 0.2, 0), (0, 0, 1), 0.13, 0.3, P["wood"], sides=10)
    for z in (0.06, 0.24):
        prism((hw + 0.05, cy - hd - 0.2, z), (0, 0, 1), 0.135, 0.025, P["iron"], sides=10)
    pillow((hw - 0.08, cy - hd - 0.33, 0.3), (hw + 0.18, cy - hd - 0.07, 0.34), P["cap"], round_=0.02)
    box((-hw - 0.2, cy - 0.1, 0), (-hw - 0.0, cy + 0.12, 0.2), P["wood"], bevel=0.015)
    box((-hw - 0.17, cy - 0.06, 0.2), (-hw - 0.02, cy + 0.08, 0.34), P["wood"], bevel=0.015)
    # Drying rack with fish.
    rack = Vector((0, -hd - 0.3, 0))
    for x in (-0.48, 0.48):
        log(rack + Vector((x, 0, 0)), rack + Vector((x, 0, 0.62)), 0.03, P["dark"], sides=6)
    log(rack + Vector((-0.52, 0, 0.58)), rack + Vector((0.52, 0, 0.58)), 0.022, P["dark"], sides=6)
    for k in range(5):
        fish(P, rack + Vector((-0.32 + k * 0.16, 0, 0.42)))
    pennant(P, (hw + 0.08, cy + hd + 0.08, 0), 1.2)
    finish("store")


def skirt(r_in, r_out, h, material, center=(0, 0), n=40, rings=4, name="drift"):
    """Snow drifted round a base: a smooth ring falling from height `h` at
    `r_in` to the ground at `r_out`."""
    cx, cy = center
    v, f = [], []
    for j in range(rings + 1):
        t = j / rings
        r = r_in + (r_out - r_in) * t
        z = h * (1 - t) ** 2 * (1 + 0.5 * t)
        for i in range(n):
            a = i / n * TAU
            v.append((cx + math.cos(a) * r, cy + math.sin(a) * r, z))
    for j in range(rings):
        for i in range(n):
            a, b = j * n + i, j * n + (i + 1) % n
            f.append((a, b, b + n, a + n))
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(v, [], f)
    return _object(name, mesh, material, True)


def hemisphere(center, r, h, material, n=20, rings=5, name="core"):
    """The top half of a squashed sphere, standing on the ground."""
    cx, cy, cz = center
    v = [(cx + math.cos(i / n * TAU) * math.cos(j / rings * math.pi / 2) * r, cy + math.sin(i / n * TAU) * math.cos(j / rings * math.pi / 2) * r, cz + math.sin(j / rings * math.pi / 2) * h) for j in range(rings) for i in range(n)]
    v.append((cx, cy, cz + h))
    f = [(j * n + i, j * n + (i + 1) % n, (j + 1) * n + (i + 1) % n, (j + 1) * n + i) for j in range(rings - 1) for i in range(n)]
    f += [((rings - 1) * n + i, (rings - 1) * n + (i + 1) % n, len(v) - 1) for i in range(n)]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(v, [], f)
    return _object(name, mesh, material, True)


def block_dome(rng, P, center, r, h, rows, cols0, material, door=None, depth=0.2, keep=None, shell=True, keystone=True):
    """A dome of snow blocks laid in rings, each ring staggered against the
    one below. `door` leaves a gap that wide (radians) at the front (-y) in
    the bottom two rings. A shaded shell inside fills the joints."""
    center = Vector(center)
    if shell:
        hemisphere(center, r - depth * 0.6, h - depth * 0.6, P["shade"])
    for row in range(rows):
        el0, el1 = row / rows * math.pi / 2, (row + 1) / rows * math.pi / 2
        el = (el0 + el1) / 2
        cols = max(5, round(math.cos(el) * cols0))
        tall = math.hypot(h * math.cos(el), r * math.sin(el)) * (el1 - el0) - 0.025
        for c in range(cols):
            az = (c + 0.5 * (row % 2)) / cols * TAU
            if door and row < 2 and abs(((az + math.pi / 2 + math.pi) % TAU) - math.pi) < door:
                continue
            if keep and not keep(row, az):
                continue
            bulge = rng.uniform(-0.012, 0.012)
            centre = center + Vector((math.cos(el) * math.cos(az) * (r + bulge), math.cos(el) * math.sin(az) * (r + bulge), math.sin(el) * (h + bulge)))
            length = TAU * math.cos(el) * r / cols - 0.025
            tangent = Vector((-math.sin(az), math.cos(az), 0))
            outward = Vector((math.cos(el) * math.cos(az) / r, math.cos(el) * math.sin(az) / r, math.sin(el) / h)).normalized()
            up = outward.cross(tangent)
            blk = obox((0, 0, 0), (length, tall, depth), (0, 0, 0), material, bevel=0.03)
            blk.matrix_world = Matrix((
                (tangent.x, up.x, outward.x, centre.x),
                (tangent.y, up.y, outward.y, centre.y),
                (tangent.z, up.z, outward.z, centre.z),
                (0, 0, 0, 1),
            ))
    # The keystone: a round cap of snow over the top.
    if keystone:
        ball(center + Vector((0, 0, h - 0.04)), 1.0, P["cap"], subdivisions=2, scale=(r * 0.24, r * 0.24, 0.12), name="snow")


def igloo(tier):
    """The tribe's igloo (4 across): a dome of snow blocks, a block tunnel
    to the door at the front, warm windows, a lantern, snowballs by the
    door and the owner's flag. Tier 1, the Snow Hall, is bigger, with a
    second dome behind and banners; tier 2, the Ice Palace, is blue ice
    ringed with spires."""
    rng = random.Random(7 + tier)
    P = palette()
    hall, palace = tier >= 1, tier >= 2
    blocks = mat("Gloss Ice Block", (170, 214, 246)) if palace else P["snow"]
    r0 = 1.75 * (1.14 if hall else 1.0)
    h0 = 1.5 * (1.16 if hall else 1.0)
    # Bigger domes are laid with fewer, bigger blocks, to keep in budget.
    block_dome(rng, P, (0, 0, 0), r0, h0, 6 if not hall else 5, 21 if not hall else 19, blocks, door=0.42, depth=0.22)
    if hall:
        block_dome(rng, P, (0, r0 * 0.7, 0), r0 * 0.6, h0 * 0.62, 3, 11, blocks, depth=0.18)
    skirt(r0 - 0.08, r0 + 0.5, 0.22, P["cap"])
    # The tunnel: arches of blocks out to the door.
    tr, tl = 0.6, 1.0
    y0 = -(r0 - 0.4)
    for seg in range(3):
        ya = y0 - seg * tl / 3
        for k in range(5):
            a = (k + 0.5) / 5 * math.pi
            c = Vector((math.cos(a) * (tr + 0.08), ya - tl / 6, math.sin(a) * (tr * 1.15 + 0.08)))
            blk = obox(c, (math.pi * tr / 5 * 1.08, tl / 3 - 0.03, 0.17), (0, a - math.pi / 2, 0), blocks, bevel=0.03)
            blk.rotation_euler = (0, -(a - math.pi / 2), 0)
    # Snow lying along the tunnel's back.
    for seg in range(3):
        ball((rng.uniform(-0.05, 0.05), y0 - (seg + 0.5) * tl / 3, tr * 1.15 + 0.17), 1.0, P["cap"], subdivisions=2, scale=(0.32, tl / 5, 0.07), name="snow")
    door_y = y0 - tl - 0.005
    arch = [(math.cos(a) * tr, door_y, math.sin(a) * tr * 1.15) for a in (i / 12 * math.pi for i in range(13))]
    poly(arch, P["hole"], "doorway")
    # A tunnel floor of trodden snow running out of the door.
    pillow((-0.5, door_y - 0.45, 0), (0.5, door_y + 0.2, 0.05), P["cap"], round_=0.04)
    # Warm windows of clear ice on either side.
    glow = mat("Glow Window", (255, 200, 120))
    for az in (math.radians(200), math.radians(-20), math.radians(110)):
        el = 0.62
        n = Vector((math.cos(el) * math.cos(az), math.cos(el) * math.sin(az), math.sin(el) * r0 / h0)).normalized()
        p = Vector((math.cos(el) * math.cos(az) * (r0 + 0.12), math.cos(el) * math.sin(az) * (r0 + 0.12), math.sin(el) * (h0 + 0.12)))
        t = Vector((-math.sin(az), math.cos(az), 0))
        u = n.cross(t)
        quad = [p + t * sx * 0.2 + u * sy * 0.15 for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
        poly([tuple(q) for q in quad], glow, "window")
    # A lantern on a post by the door.
    lp = Vector((-tr - 0.38, door_y + 0.1, 0))
    log(lp, lp + Vector((0, 0, 1.0)), 0.035, P["dark"], sides=6)
    log(lp + Vector((0, 0, 0.95)), lp + Vector((0.22, 0, 0.95)), 0.022, P["dark"], sides=5)
    box(lp + Vector((0.16, -0.06, 0.66)), lp + Vector((0.28, 0.06, 0.84)), P["lamp"], bevel=0.015)
    box(lp + Vector((0.15, -0.07, 0.84)), lp + Vector((0.29, 0.07, 0.88)), P["iron"])
    # Snowballs ready by the door, and a fishing spear left against the wall.
    for i, (x, y, z) in enumerate([(0.9, -r0 - 0.15, 0), (1.12, -r0 - 0.05, 0), (1.0, -r0 + 0.12, 0), (1.0, -r0 - 0.03, 0.15)]):
        ball((x, y, 0.09 + z), 0.1, P["cap"], subdivisions=1)
    log((-r0 * 0.72, -r0 * 0.6, 0.02), (-r0 * 0.66 + 0.1, -r0 * 0.56, 1.3), 0.02, P["wood"], sides=5)
    cone((-r0 * 0.66 + 0.1, -r0 * 0.56, 1.28), (0.08, 0.03, 1), 0.035, 0.16, P["iron"], sides=5)
    # The owner's flag beside the door.
    pole = Vector((0.95 if not hall else 1.15, -r0 - 0.4, 0))
    prism(pole, (0, 0, 1), 0.04, 2.4, P["dark"], sides=6)
    ball(pole + Vector((0, 0, 2.45)), 0.08, mat("Metal Gold", (230, 190, 60)), subdivisions=1)
    for i in range(5):
        x0, x1 = 0.04 + i * 0.18, 0.04 + (i + 1) * 0.18
        w0, w1 = math.sin(x0 * 4) * 0.07, math.sin(x1 * 4) * 0.07
        poly([tuple(pole + Vector(p)) for p in ((x0, w0, 1.8), (x1, w1, 1.8 + 0.01 * i), (x1, w1, 2.35), (x0, w0, 2.35))], P["team"], "flag")
    if hall:
        for sx in (-1, 1):
            bp = Vector((sx * (r0 + 0.45), -r0 * 0.5, 0))
            prism(bp, (0, 0, 1), 0.05, 2.6, P["dark"], sides=6)
            log(bp + Vector((-0.28, 0, 2.48)), bp + Vector((0.28, 0, 2.48)), 0.025, P["dark"], sides=5)
            poly([tuple(bp + Vector(p)) for p in ((-0.25, -0.02, 2.46), (0.25, -0.02, 2.46), (0.25, -0.02, 1.7), (0, -0.02, 1.56), (-0.25, -0.02, 1.7))], P["team"], "banner")
            ball(bp + Vector((0, 0, 2.66)), 0.09, mat("Metal Gold", (230, 190, 60)), subdivisions=1)
    if palace:
        spire = mat("Gloss Ice Spire", (210, 236, 255))
        for i in range(6):
            a = i / 6 * TAU + 0.5
            if abs(((a + math.pi / 2 + math.pi) % TAU) - math.pi) < 0.5:
                continue
            p = (math.cos(a) * (r0 + 0.32), math.sin(a) * (r0 + 0.32), 0)
            cone(p, (0, 0, 1), 0.2, 1.0 + (i % 2) * 0.5, spire, sides=5)
        for i in range(3):
            a = i / 3 * TAU + 0.9
            cone((math.cos(a) * 0.45, math.sin(a) * 0.45, h0 - 0.2), (math.cos(a) * 0.15, math.sin(a) * 0.15, 1), 0.14, 0.7, spire, sides=5)
    finish(f"igloo-{tier}", stages=False)


# --------------------------------------------------------------------------
# Camps (2x2): each looks like what it trains.


def shield(P, at, face=(0, -1, 0), r=0.2):
    """A round shield of planks with an iron rim and the owner's boss."""
    at, face = Vector(at), Vector(face).normalized()
    prism(at, face, r, 0.04, P["wood"], sides=10, smooth=False, name="shield")
    prism(at - face * 0.005, face, r + 0.015, 0.03, P["iron"], sides=10, smooth=False, name="rim")
    ball(at + face * 0.05, 0.065, P["team"], subdivisions=1, name="boss")


def ski(P, at, along, length=0.9, up=False):
    """A ski with an upturned tip, lying along `along` or stood on its tail."""
    at, along = Vector(at), Vector(along).normalized()
    if up:
        log(at, at + Vector((along.x * 0.08, along.y * 0.08, length)), 0.028, P["team"], sides=4)
        return
    log(at, at + along * length, 0.03, P["team"], sides=4)
    log(at + along * length, at + along * (length + 0.08) + Vector((0, 0, 0.07)), 0.03, P["team"], sides=4)


def sled(P, at, along=(0, -1, 0)):
    """A wooden sled: a plank bed on two runners curling up at the front."""
    at, a = Vector(at), Vector(along).normalized()
    side = Vector((-a.y, a.x, 0))
    for k in (-1, 1):
        o = at + side * k * 0.2
        log(o - a * 0.45 + Vector((0, 0, 0.03)), o + a * 0.35 + Vector((0, 0, 0.03)), 0.022, P["iron"], sides=5)
        log(o + a * 0.35 + Vector((0, 0, 0.03)), o + a * 0.5 + Vector((0, 0, 0.18)), 0.022, P["iron"], sides=5)
        for t in (-0.3, 0.2):
            log(o + a * t + Vector((0, 0, 0.03)), o + a * t + Vector((0, 0, 0.14)), 0.02, P["dark"], sides=5)
    # The bed: planks across the runners.
    for i in range(4):
        c = at + a * (-0.32 + i * 0.22) + Vector((0, 0, 0.16))
        obox(c, (0.54, 0.2, 0.04), (0, 0, math.atan2(a.y, a.x) - math.pi / 2), P["wood"], bevel=0.01)
    log(at + a * 0.5 + side * -0.2 + Vector((0, 0, 0.18)), at + a * 0.5 + side * 0.2 + Vector((0, 0, 0.18)), 0.02, P["dark"], sides=5)


def plank_roof(P, rng, x0, x1, y0, y1, eave, ridge, snow=True):
    """A pitched roof, ridge along x, with snow lying thick on both sides."""
    cy = (y0 + y1) / 2
    half = (y1 - y0) / 2
    pitch = math.atan2(ridge - eave, half)
    slope = math.hypot(half, ridge - eave) + 0.03
    for sgn in (-1, 1):
        mid = Vector(((x0 + x1) / 2, cy + sgn * half / 2, (eave + ridge) / 2 + 0.02))
        obox(mid, (x1 - x0, slope, 0.05), (-sgn * pitch, 0, 0), P["dark"], bevel=0.01)
        if snow:
            obox(mid + Vector((0, sgn * 0.01, 0.06)), (x1 - x0 - 0.06, slope - 0.08, 0.08), (-sgn * pitch, 0, 0), P["cap"], bevel=0.035, segments=2, smooth=True)
    log((x0 - 0.04, cy, ridge + 0.02), (x1 + 0.04, cy, ridge + 0.02), 0.04, P["wood"])
    icicles(rng, P, [(rng.uniform(x0 + 0.05, x1 - 0.05), y0 + 0.02, eave - 0.02) for _ in range(4)])


def snowballs(P, rng, at, n, r=0.08):
    at = Vector(at)
    for i in range(n):
        a = i * 2.4
        d = 0.0 if i == 0 else r * 1.9 * (1 + i // 7)
        ball(tuple(at + Vector((math.cos(a) * d, math.sin(a) * d, r + (0.12 if i >= 7 else 0)))), r, P["cap"], subdivisions=1)


def workshop():
    """Workshop (Shieldbearers): an open-fronted timber shed, a workbench
    with a shield on it, shields hung on a rack and a sawhorse."""
    rng = random.Random(61)
    P = palette()
    yard(rng, P, fence_sides="back", crates=1)
    hw, y0, y1, eave = 0.8, -0.2, 0.8, 0.85
    for x in (-hw, hw):
        for y in (y0, y1):
            log((x, y, 0), (x, y, eave), 0.05, P["wood"])
    # Back and side walls of logs; the front is open.
    for k in range(5):
        z = 0.06 + k * 0.15
        log((-hw, y1, z), (hw, y1, z), 0.06, P["wood"])
        for x in (-hw, hw):
            log((x, y0, z), (x, y1, z), 0.06, P["wood"])
    box((-hw, y0, 0), (hw, y1, 0.03), P["dark"], name="core")
    plank_roof(P, rng, -hw - 0.12, hw + 0.12, y0 - 0.15, y1 + 0.15, eave, eave + 0.38)
    # Workbench under the roof with a shield half made.
    box((-0.5, 0.25, 0.38), (0.3, 0.6, 0.45), P["wood"], bevel=0.01)
    for x in (-0.45, 0.25):
        for y in (0.3, 0.55):
            log((x, y, 0), (x, y, 0.38), 0.025, P["dark"], sides=5)
    shield(P, (-0.1, 0.42, 0.47), face=(0, 0, 1), r=0.17)
    box((0.05, 0.3, 0.45), (0.25, 0.36, 0.48), P["iron"])
    # A rack of finished shields out front.
    for x in (-0.65, 0.65):
        log((x, -0.62, 0), (x, -0.62, 0.62), 0.03, P["dark"], sides=6)
    log((-0.7, -0.62, 0.58), (0.7, -0.62, 0.58), 0.025, P["dark"], sides=6)
    for k, x in enumerate((-0.42, 0.0, 0.42)):
        shield(P, (x, -0.66, 0.36), face=(0, -1, 0.15), r=0.19)
    # Sawhorse with a plank on it.
    log((0.95, -0.15, 0), (0.95, 0.15, 0.3), 0.022, P["dark"], sides=5)
    log((0.95, 0.15, 0), (0.95, -0.15, 0.3), 0.022, P["dark"], sides=5)
    box((0.88, -0.1, 0.3), (1.05, 0.5, 0.34), P["wood"])
    pennant(P, (-hw - 0.15, -0.3, 0), 1.35)
    finish("workshop")


def sling_range():
    """Sling Range (Slingers): two painted targets on posts at the back, a
    throwing line of snow at the front, slings on a rack, a ball basket."""
    rng = random.Random(71)
    P = palette()
    yard(rng, P, crates=1, barrel=False)
    red = mat("Team Cloth Ring", (255, 255, 255))
    for x, h in ((-0.5, 0.75), (0.45, 0.6)):
        log((x, 0.7, 0), (x, 0.7, h), 0.035, P["dark"], sides=6)
        for k, (rr, m) in enumerate(((0.26, P["cap"]), (0.19, red), (0.12, P["cap"]), (0.055, red))):
            prism((x, 0.7 - 0.04 - k * 0.012, h + 0.22), (0, -1, 0), rr, 0.04, m, sides=14, smooth=False, name="target")
        # Snowballs splatted on the target.
        for _ in range(2):
            ball((x + rng.uniform(-0.15, 0.15), 0.62, h + 0.22 + rng.uniform(-0.15, 0.15)), 0.035, P["cap"], subdivisions=1)
    # Throwing line: a low ridge of snow blocks.
    for k in range(4):
        x = -0.8 + k * 0.42
        box((x, -0.55, 0), (x + 0.38, -0.4, 0.16), P["snow"], bevel=0.03)
    pillow((-0.85, -0.6, 0.14), (0.85, -0.36, 0.2), P["cap"], round_=0.03)
    # Sling rack: a crossbar with slings hanging.
    for y in (-0.15, 0.25):
        log((-0.95, y, 0), (-0.95, y, 0.7), 0.025, P["dark"], sides=5)
    log((-0.95, -0.2, 0.66), (-0.95, 0.3, 0.66), 0.02, P["dark"], sides=5)
    for k in range(3):
        y = -0.08 + k * 0.14
        log((-0.95, y, 0.66), (-0.97, y, 0.32), 0.008, P["rope"], sides=4)
        ball((-0.97, y, 0.3), 0.03, P["rope"], subdivisions=1, scale=(0.6, 1, 1.3))
    # A basket of snowballs.
    prism((0.85, -0.25, 0), (0, 0, 1), 0.17, 0.18, P["rope"], sides=10)
    snowballs(P, rng, (0.85, -0.25, 0.12), 5, r=0.06)
    pennant(P, (0.9, 0.85, 0), 1.2)
    finish("sling-range")


def ski_lodge():
    """Ski Lodge (Skiers): a steep A-frame cabin, skis stood in a rack, and
    a little jump of packed snow marked with flags."""
    rng = random.Random(81)
    P = palette()
    yard(rng, P, crates=1)
    hw, y0, y1, top = 0.6, -0.1, 0.85, 1.35
    slope = math.hypot(hw, top) + 0.05
    for sgn in (-1, 1):
        # Each roof runs from the eave at x = sgn * hw up to the ridge.
        turn = math.atan2(-top, -sgn * hw)
        out = Vector((sgn * top, 0, hw)).normalized()
        mid = Vector((sgn * hw / 2, (y0 + y1) / 2, top / 2))
        obox(mid + out * 0.03, (slope, y1 - y0 + 0.15, 0.06), (0, turn, 0), P["dark"], bevel=0.01)
        obox(mid + out * 0.09, (slope - 0.15, y1 - y0 + 0.05, 0.07), (0, turn, 0), P["cap"], bevel=0.03, segments=2, smooth=True)
    # The front gable: planks, a door and a round window.
    gable = [(-hw + 0.05, y0, 0), (hw - 0.05, y0, 0), (0, y0, top - 0.08)]
    poly(gable, P["wood"], "gable")
    poly([(-hw + 0.05, y1, 0), (hw - 0.05, y1, 0), (0, y1, top - 0.08)], P["wood"], "gable")
    box((-0.17, y0 - 0.03, 0), (0.17, y0 + 0.01, 0.5), P["hole"])
    box((-0.21, y0 - 0.05, 0.5), (0.21, y0 + 0.01, 0.55), P["dark"])
    prism((0, y0 + 0.005, 0.8), (0, -1, 0), 0.1, 0.02, P["lamp"], sides=10, smooth=False, name="window")
    log((0, y0 - 0.03, top - 0.05), (0, y0 - 0.25, top + 0.12), 0.025, P["dark"], sides=5)
    # Ski rack beside the door, skis stood up in it.
    log((0.68, -0.6, 0.45), (0.68, 0.2, 0.45), 0.025, P["dark"], sides=5)
    for y in (-0.6, 0.2):
        log((0.68, y, 0), (0.68, y, 0.48), 0.025, P["dark"], sides=5)
    for k in range(4):
        ski(P, (0.72, -0.5 + k * 0.2, 0), (1, 0, 0), length=1.0, up=True)
    # Poles crossed in the snow.
    log((-0.8, -0.5, 0), (-0.7, -0.4, 0.7), 0.012, P["iron"], sides=4)
    log((-0.62, -0.5, 0), (-0.72, -0.4, 0.7), 0.012, P["iron"], sides=4)
    # A small jump at the front, flagged.
    ramp = [(-0.55, -0.6, 0), (-0.05, -0.6, 0), (-0.05, -0.95, 0), (-0.55, -0.95, 0), (-0.55, -0.6, 0.22), (-0.05, -0.6, 0.22)]
    mesh = bpy.data.meshes.new("jump")
    mesh.from_pydata(ramp, [], [(0, 1, 5, 4), (3, 2, 1, 0), (3, 0, 4), (2, 5, 1), (2, 3, 4, 5)])
    _object("jump", mesh, P["cap"], True)
    for x in (-0.6, 0.0):
        log((x, -0.62, 0), (x, -0.62, 0.45), 0.012, P["dark"], sides=4)
        poly([(x, -0.62, 0.45), (x + 0.14, -0.62, 0.4), (x, -0.62, 0.35)], P["team"], "flag")
    icicles(rng, P, [(sx * rng.uniform(0.3, 0.55), rng.uniform(0, 0.7), 0.4) for sx in (-1, 1) for _ in range(2)])
    pennant(P, (-0.85, 0.75, 0), 1.3)
    finish("ski-lodge")


def packer_yard():
    """Packer Yard (Packers): a giant snowball rolled and packed, a timber
    press, more balls waiting, and shovels stuck in the snow."""
    rng = random.Random(91)
    P = palette()
    yard(rng, P, fence_sides="u", crates=2, barrel=False)
    ball((-0.3, 0.2, 0.45), 0.45, P["cap"], subdivisions=3, name="snow")
    # Packing bands round it.
    for z in (0.3, 0.6):
        prism((-0.3, 0.2, z), (0, 0, 1), 0.44 * math.sqrt(1 - ((z - 0.45) / 0.45) ** 2) + 0.01, 0.04, P["rope"], sides=14)
    ball((0.55, 0.45, 0.28), 0.28, P["cap"], subdivisions=2, name="snow")
    ball((0.6, -0.15, 0.2), 0.2, P["cap"], subdivisions=2, name="snow")
    # The press: an A-frame over a packing pit.
    for x in (-0.85, 0.25):
        log((x, -0.75, 0), (x + (0.2 if x < 0 else -0.2), -0.75, 1.1), 0.04, P["wood"])
        log((x, -0.25, 0), (x + (0.2 if x < 0 else -0.2), -0.25, 1.1), 0.04, P["wood"])
    log((-0.7, -0.8, 1.08), (0.1, -0.8, 1.08), 0.045, P["dark"])
    log((-0.7, -0.2, 1.08), (0.1, -0.2, 1.08), 0.045, P["dark"])
    log((-0.3, -0.85, 1.12), (-0.3, -0.15, 1.12), 0.05, P["wood"])
    log((-0.3, -0.5, 1.12), (-0.3, -0.5, 0.5), 0.012, P["rope"], sides=4)
    box((-0.45, -0.65, 0.3), (-0.15, -0.35, 0.5), P["wood"], bevel=0.02)
    prism((-0.3, -0.5, 0), (0, 0, 1), 0.3, 0.12, P["snow"], sides=12)
    # Shovels stuck in the snow.
    for x, lean in ((0.6, 0.15), (0.85, -0.1)):
        log((x, -0.65, 0.2), (x + lean, -0.6, 0.85), 0.02, P["wood"], sides=5)
        obox((x, -0.66, 0.12), (0.16, 0.03, 0.2), (0, -lean, 0), P["iron"])
    pennant(P, (0.9, 0.85, 0), 1.3)
    finish("packer-yard")


def spa():
    """Spa (Warmers): a hot pool ringed with stones, steam rising, a small
    snow hut to change in and towels on a line."""
    rng = random.Random(101)
    P = palette()
    yard(rng, P, crates=0, barrel=False, tracks=False)
    water = mat("Glow Gloss Water", (120, 200, 230))
    prism((-0.15, -0.15, 0), (0, 0, 1), 0.6, 0.06, water, sides=16)
    for k in range(14):
        a = k / 14 * TAU
        stone(rng, P, (-0.15 + math.cos(a) * 0.66, -0.15 + math.sin(a) * 0.66, 0.06), 0.11)
    steam = mat("Steam", (245, 248, 252))
    for k in range(5):
        a = k * 1.3
        d = rng.uniform(0.1, 0.35)
        ball((-0.15 + math.cos(a) * d, -0.15 + math.sin(a) * d, 0.3 + k * 0.12), 0.1 + k * 0.025, steam, subdivisions=1)
    # A snow hut at the back corner.
    block_dome(rng, P, (0.55, 0.55, 0), 0.42, 0.55, 3, 9, P["snow"], depth=0.12)
    box((0.3, 0.12, 0), (0.55, 0.2, 0.32), P["hole"])
    # A line of towels in the owner's colours.
    for x in (-0.9, 0.0):
        log((x, 0.75, 0), (x, 0.75, 0.8), 0.025, P["dark"], sides=5)
    log((-0.9, 0.75, 0.75), (0.0, 0.75, 0.75), 0.008, P["rope"], sides=4)
    for k in range(3):
        x = -0.78 + k * 0.27
        poly([(x, 0.74, 0.75), (x + 0.2, 0.74, 0.75), (x + 0.2, 0.74, 0.45), (x, 0.74, 0.48)], P["team"] if k != 1 else P["cap"], "towel")
    # A bucket and ladle.
    prism((0.6, -0.65, 0), (0, 0, 1), 0.11, 0.18, P["wood"], sides=10)
    prism((0.6, -0.65, 0.05), (0, 0, 1), 0.115, 0.025, P["iron"], sides=10)
    log((0.6, -0.65, 0.12), (0.75, -0.6, 0.35), 0.012, P["dark"], sides=4)
    pennant(P, (-0.9, -0.85, 0), 1.2)
    finish("spa")


def track():
    """Track (Sleds): a sled shed with a sled out front and a curve of
    track marked with flagged posts."""
    rng = random.Random(111)
    P = palette()
    yard(rng, P, crates=1, barrel=True)
    x0, x1, y0, y1, eave = -0.85, 0.2, 0.05, 0.85, 0.65
    for x in (x0, x1):
        for y in (y0, y1):
            log((x, y, 0), (x, y, eave), 0.045, P["wood"])
    for k in range(4):
        z = 0.06 + k * 0.15
        log((x0, y1, z), (x1, y1, z), 0.055, P["wood"])
        log((x0, y0, z), (x0, y1, z), 0.055, P["wood"])
    plank_roof(P, rng, x0 - 0.1, x1 + 0.1, y0 - 0.12, y1 + 0.12, eave, eave + 0.32)
    sled(P, (-0.35, 0.45, 0), along=(0, -1, 0))
    sled(P, (-0.25, -0.5, 0), along=(1, -0.4, 0))
    # The track: trodden ruts curving off, with flagged posts.
    for k in range(6):
        a = -0.4 + k * 0.32
        c = Vector((0.55 + math.cos(a) * 0.35, -0.1 + math.sin(a) * 0.55, 0))
        ball(tuple(c), 1.0, P["cap"], subdivisions=1, scale=(0.12, 0.12, 0.03), name="snow")
    for k, (x, y) in enumerate(((0.95, -0.7), (1.0, 0.1), (0.75, 0.7))):
        log((x, y, 0), (x, y, 0.5), 0.015, P["dark"], sides=4)
        poly([(x, y, 0.5), (x + 0.15, y, 0.45), (x, y, 0.4)], P["team"], "flag")
    pennant(P, (x0 - 0.1, -0.2, 0), 1.25)
    finish("track")


def bunkhouse():
    """Bunkhouse: a long vault of snow blocks with a plank door at the
    front, warm windows down the sides, a stovepipe, and supplies stacked."""
    rng = random.Random(121)
    P = palette()
    r, length = 0.62, 1.6
    y0 = -length / 2
    box((-r + 0.15, y0 + 0.05, 0), (r - 0.15, -y0 - 0.05, r - 0.05), P["shade"], name="core")
    rings = 6
    for seg in range(rings):
        y = y0 + (seg + 0.5) * length / rings
        n = 6
        for k in range(n):
            a = (k + 0.5 * (seg % 2) + 0.25) / (n + 0.5) * math.pi
            c = Vector((math.cos(a) * r, y, math.sin(a) * r * 1.1))
            obox(c, (math.pi * r / n * 1.05, length / rings - 0.025, 0.16), (0, -(a + math.pi / 2), 0), P["snow"], bevel=0.03)
    for y in (y0, -y0):
        poly([(math.cos(a) * (r - 0.02), y, math.sin(a) * (r - 0.02) * 1.1) for a in (i / 10 * math.pi for i in range(11))], P["snow"], "end")
    # Snow along the ridge.
    ball((0, 0, r * 1.1 + 0.1), 1.0, P["cap"], subdivisions=2, scale=(0.3, length / 2 + 0.02, 0.09), name="snow")
    # Front door of planks, with a lamp.
    f = y0 - 0.02
    box((-0.22, f - 0.02, 0), (0.22, f + 0.02, 0.55), P["dark"])
    for i in range(3):
        x = -0.18 + i * 0.12
        box((x + 0.005, f - 0.05, 0.02), (x + 0.115, f - 0.01, 0.5), P["wood"], bevel=0.008)
    box((0.26, f - 0.12, 0.45), (0.34, f - 0.04, 0.57), P["lamp"], bevel=0.01)
    # Windows down each side.
    for sx in (-1, 1):
        for y in (-0.3, 0.35):
            a = math.pi / 2 - sx * 0.95
            p = Vector((math.cos(a) * (r + 0.09), y, math.sin(a) * (r + 0.09) * 1.1))
            n = Vector((math.cos(a), 0, math.sin(a)))
            t = Vector((0, 1, 0))
            u = n.cross(t)
            poly([tuple(p + t * dx * 0.1 + u * dy * 0.08) for dx, dy in ((-1, -1), (1, -1), (1, 1), (-1, 1))], mat("Glow Window", (255, 200, 120)), "window")
    # Stovepipe out the back.
    prism((0.25, 0.5, 0.5), (0, 0, 1), 0.05, 0.4, P["iron"], sides=8)
    # Crates and a barrel by the door.
    box((-0.95, -0.95, 0), (-0.65, -0.7, 0.22), P["wood"], bevel=0.015)
    box((-0.9, -0.92, 0.22), (-0.7, -0.74, 0.38), P["wood"], bevel=0.015)
    prism((0.8, -0.8, 0), (0, 0, 1), 0.12, 0.28, P["wood"], sides=10)
    snowballs(P, rng, (0.75, 0.6, 0), 4, r=0.07)
    pillow((-r - 0.2, y0, 0), (r + 0.2, -y0, 0.12), P["cap"], round_=0.1)
    pennant(P, (-0.85, 0.75, 0), 1.3)
    finish("bunkhouse")


# --------------------------------------------------------------------------
# Scenery: trees, stumps, ice, fishing holes and the ruin.
# None of these are built, so none has construction stages.


def needles(k):
    return mat(f"Pine Needles {k}", [(38, 92, 70), (44, 104, 76), (34, 84, 66), (52, 112, 82)][k])


def pine(seed):
    """A snowy fir: tiers of drooping, jagged branch skirts on a short trunk,
    each tier heavy with snow on top and dark green beneath. Variant 0 is
    a tall slim fir, 1 a broad bushy one, 2 a young one bent under snow.
    About two tiles tall."""
    rng = random.Random(seed * 7 + 3)
    P = palette()
    bark = mat("Wood Bark", (92, 64, 46))
    tiers, width, height = [(5, 0.62, 2.2), (4, 0.78, 1.9), (4, 0.55, 1.6)][seed]
    log((0, 0, 0), (0, 0, 0.55), 0.08, bark, sides=6)
    lean = Vector((rng.uniform(-0.05, 0.05), rng.uniform(-0.05, 0.05), 0))
    spin = rng.uniform(0, TAU)
    for t in range(tiers):
        f = t / (tiers - 1)
        r = width * (1 - 0.68 * f)
        rim = 0.35 + (height - 0.8) * f
        apex = Vector((0, 0, rim + 0.6 - 0.15 * f)) + lean * f * 3
        n = 9 - min(t, 3)
        pts = []
        for i in range(n * 2):
            a = i / (n * 2) * TAU + spin + t * 0.6
            tip = i % 2 == 0
            rr = r * rng.uniform(0.9, 1.1) if tip else r * 0.6
            z = rim - (0.12 if tip else -0.05) + (rng.uniform(-0.03, 0.03) if tip else 0)
            pts.append(Vector((math.cos(a) * rr, math.sin(a) * rr, z)) + lean * f * 3)
        under = Vector((0, 0, rim + 0.04)) + lean * f * 3
        verts = [tuple(apex), tuple(under)] + [tuple(p) for p in pts]
        faces = [(0, 2 + i, 2 + (i + 1) % (n * 2)) for i in range(n * 2)]
        faces += [(1, 2 + (i + 1) % (n * 2), 2 + i) for i in range(n * 2)]
        mesh = bpy.data.meshes.new("branches")
        mesh.from_pydata(verts, [], faces)
        _object("branches", mesh, needles(rng.randrange(4)), False)
        # Snow lying on the tier: the upper part of the skirt, lifted, with
        # a ragged edge and clumps sagging toward the branch tips.
        cover = rng.uniform(0.55, 0.72) + (0.12 if seed == 2 else 0)
        cap = []
        for i, p in enumerate(pts):
            k = cover * (rng.uniform(0.95, 1.2) if i % 2 == 0 else rng.uniform(0.7, 0.95))
            cap.append(apex + (p - apex) * k + Vector((0, 0, 0.04)))
        verts = [tuple(apex + Vector((0, 0, 0.04)))] + [tuple(c) for c in cap]
        faces = [(0, 1 + i, 1 + (i + 1) % (n * 2)) for i in range(n * 2)]
        mesh = bpy.data.meshes.new("snow")
        mesh.from_pydata(verts, [], faces)
        _object("snow", mesh, P["cap"], True)
    # Snow heaped round the foot of the trunk.
    skirt(0.08, width * 0.75, 0.1, P["cap"], n=12, rings=2)
    finish(f"tree-{seed}", stages=False)


def stump(seed):
    """A felled tree's stump: a sawn top with rings, snow on half of it,
    roots reaching out, chips and a split log beside it."""
    rng = random.Random(seed * 13 + 5)
    P = palette()
    bark = mat("Wood Bark", (92, 64, 46))
    cut = mat("Wood Cut", (204, 168, 118))
    h = [0.26, 0.2, 0.32][seed]
    prism((0, 0, 0), (0, 0, 1), 0.17, h, bark, sides=8, smooth=False)
    prism((0, 0, h), (0, 0, 1), 0.165, 0.01, cut, sides=8, smooth=False)
    prism((0, 0, h + 0.006), (0, 0, 1), 0.09, 0.008, mat("Wood Ring", (170, 130, 86)), sides=8, smooth=False)
    ball((0.05, 0.04, h + 0.02), 1.0, P["cap"], subdivisions=1, scale=(0.12, 0.1, 0.035), name="snow")
    for k in range(4):
        a = k / 4 * TAU + rng.uniform(-0.4, 0.4)
        log((math.cos(a) * 0.12, math.sin(a) * 0.12, 0.1), (math.cos(a) * 0.3, math.sin(a) * 0.3, 0.0), 0.04, bark, sides=5)
    for _ in range(4):
        a, d = rng.uniform(0, TAU), rng.uniform(0.25, 0.4)
        obox((math.cos(a) * d, math.sin(a) * d, 0.01), (0.07, 0.03, 0.015), (0, 0, rng.uniform(0, TAU)), cut)
    if seed == 1:
        log((0.3, -0.2, 0.06), (0.3, 0.22, 0.06), 0.06, bark, sides=6)
    if seed == 2:
        # An axe left in the stump.
        log((0.02, 0, h + 0.02), (0.22, 0.06, h + 0.3), 0.015, P["wood"], sides=5)
        obox((0.0, -0.01, h + 0.03), (0.09, 0.02, 0.07), (0, 0.6, 0.3), P["iron"])
    skirt(0.17, 0.32, 0.05, P["cap"], n=12, rings=1)
    finish(f"stump-{seed}", stages=False)


def crystal(size, k):
    """A cluster of glowing crystal shards, worn down as it is mined
    (size 0 is a few stubs, 1 the whole cluster), in a ring of rubble."""
    rng = random.Random(5)
    P = palette()
    glow = mat("Glow Crystal", (196, 92, 236))
    pale = mat("Glow Crystal Pale", (226, 160, 250))
    shards = [(0, 0, 0.0, 0.85, 0.11)] + [(rng.uniform(0, TAU), 0.2, rng.uniform(0.3, 0.55), 0, 0.075) for _ in range(6)]
    for i, (a, off, ln, l0, r) in enumerate(shards):
        ln = (l0 or ln) * (0.25 + 0.75 * size)
        base = Vector((math.cos(a) * off, math.sin(a) * off, 0))
        axis = Vector((math.cos(a) * off * 1.6, math.sin(a) * off * 1.6, 1)).normalized()
        m = pale if i % 3 == 1 else glow
        prism(base, axis, r * (0.7 + 0.3 * size), ln * 0.75, m, sides=6, smooth=False)
        cone(base + axis * ln * 0.75, axis, r * (0.7 + 0.3 * size), ln * 0.3, m, sides=6)
    for _ in range(7):
        a, d = rng.uniform(0, TAU), rng.uniform(0.28, 0.42)
        stone(rng, P, (math.cos(a) * d, math.sin(a) * d, 0.02), 0.06)
    for _ in range(int(3 * (1 - size)) + 1):
        a, d = rng.uniform(0, TAU), rng.uniform(0.2, 0.4)
        cone((math.cos(a) * d, math.sin(a) * d, 0), (rng.uniform(-1, 1), rng.uniform(-1, 1), 0.3), 0.03, 0.1, glow, sides=5)
    skirt(0.2, 0.45, 0.05, P["cap"], n=14, rings=1)
    finish(f"crystal-{k}", stages=False)


def pinnacle(seed):
    """Jagged spires of blue ice that no one can pass, snow banked at
    their feet."""
    rng = random.Random(seed * 3 + 1)
    P = palette()
    ice = [mat("Gloss Ice Spire", (170, 215, 245)), mat("Gloss Ice Deep", (130, 186, 232)), mat("Gloss Ice Pale", (210, 236, 252))]
    for i in range(rng.randrange(4, 7)):
        p = Vector((rng.uniform(-0.3, 0.3), rng.uniform(-0.3, 0.3), 0)) if i else Vector((0, 0, 0))
        axis = Vector((p.x * 0.6 + rng.uniform(-0.1, 0.1), p.y * 0.6 + rng.uniform(-0.1, 0.1), 1)).normalized()
        ln = rng.uniform(1.1, 1.6) if i == 0 else rng.uniform(0.4, 1.0)
        r = rng.uniform(0.14, 0.22) if i == 0 else rng.uniform(0.08, 0.16)
        m = ice[i % 3]
        prism(p, axis, r, ln * 0.65, m, sides=5, smooth=False)
        cone(p + axis * ln * 0.65, axis + Vector((rng.uniform(-0.2, 0.2), rng.uniform(-0.2, 0.2), 0)), r, ln * 0.35, m, sides=5)
    skirt(0.25, 0.55, 0.14, P["cap"], n=14, rings=2)
    finish(f"pinnacle-{seed}", stages=False)


def fish_hole(open_):
    """A fishing hole: dark water in a ring of broken ice and snow, a fish
    flipping out of it; or, fished out, frozen over under a pale lid."""
    rng = random.Random(11)
    P = palette()
    r = 0.42
    if open_:
        prism((0, 0, 0), (0, 0, 1), r, 0.02, mat("Gloss Water", (18, 56, 88)), sides=20, smooth=True)
        prism((0, 0, 0), (0, 0, 1), r * 0.55, 0.025, mat("Gloss Water Deep", (10, 34, 60)), sides=14, smooth=True)
        for i in range(10):
            a = i / 10 * TAU + rng.uniform(-0.2, 0.2)
            p = (math.cos(a) * (r + 0.07), math.sin(a) * (r + 0.07), 0)
            obox(p, (rng.uniform(0.14, 0.22), rng.uniform(0.1, 0.16), rng.uniform(0.06, 0.12)), (rng.uniform(-0.3, 0.3), rng.uniform(-0.3, 0.3), a), P["ice"], bevel=0.02)
        # A fish leaping, and a ripple.
        ball((0.12, 0.05, 0.16), 1.0, mat("Gloss Fish", (150, 170, 190)), subdivisions=1, scale=(0.1, 0.035, 0.045))
        poly([(0.21, 0.05, 0.19), (0.29, 0.05, 0.27), (0.3, 0.05, 0.13)], mat("Gloss Fish", (150, 170, 190)), "tail")
        prism((0.12, 0.05, 0.02), (0, 0, 1), 0.12, 0.012, P["cap"], sides=12)
        prism((0.12, 0.05, 0.02), (0, 0, 1), 0.09, 0.016, mat("Gloss Water", (18, 56, 88)), sides=12)
    else:
        ball((0, 0, 0.0), 1.0, mat("Gloss Ice Lid", (158, 200, 228)), subdivisions=2, scale=(r, r, 0.06))
        for k in range(3):
            a = k * 2.1 + 0.4
            obox((math.cos(a) * 0.15, math.sin(a) * 0.15, 0.06), (0.26, 0.012, 0.006), (0, 0, a + 0.4), mat("Gloss Ice Crack", (230, 244, 255)))
    skirt(r, r + 0.2, 0.08, P["cap"], n=20, rings=2)
    finish("hole-open" if open_ else "hole-frozen", stages=False)


def ruin():
    """A fallen igloo: the dome broken open at the back, ragged rings of
    blocks left standing, blocks tumbled all round, the door snowed up,
    and the owner's flagpole snapped."""
    rng = random.Random(17)
    P = palette()
    r0, h0 = 1.75 * 1.06, 1.5
    def keep(row, az):
        back = abs(((az - math.pi / 2 + math.pi) % TAU) - math.pi)
        rag = 0.5 + 0.5 * math.sin(az * 3.1) * 0.6 + 0.4 * math.sin(az * 5.3)
        standing = 0 if back < 0.8 else int(1.5 + rag * 3.2)
        return row < standing
    block_dome(rng, P, (0, 0, 0), r0, h0, 6, 21, P["snow"], depth=0.22, keep=keep, shell=False, keystone=False)
    # The floor inside, dark and drifted with snow.
    prism((0, 0, 0), (0, 0, 1), r0 - 0.2, 0.03, P["shade"], sides=20)
    ball((0.3, 0.4, 0), 1.0, P["cap"], subdivisions=2, scale=(0.9, 0.7, 0.25), name="snow")
    # Fallen blocks.
    for _ in range(18):
        a = rng.uniform(0, TAU)
        d = rng.uniform(0.4, r0 + 0.8) if abs(((a - math.pi / 2 + math.pi) % TAU) - math.pi) < 1.2 else rng.uniform(r0 + 0.2, r0 + 0.7)
        obox((math.cos(a) * d, math.sin(a) * d, 0.08), (0.42, 0.3, 0.2), (rng.uniform(-0.4, 0.4), rng.uniform(-0.4, 0.4), rng.uniform(0, TAU)), P["snow"], bevel=0.03)
    skirt(r0 - 0.1, r0 + 0.45, 0.2, P["cap"])
    # The door buried in a drift.
    ball((0, -r0 - 0.1, 0), 1.0, P["cap"], subdivisions=2, scale=(0.75, 0.6, 0.5), name="snow")
    # The flagpole snapped, its flag in the snow.
    pole = Vector((0.95, -r0 - 0.35, 0))
    log(pole, pole + Vector((0.05, 0, 0.9)), 0.04, P["dark"], sides=6)
    log(pole + Vector((0.2, 0.1, 0.03)), pole + Vector((1.3, 0.5, 0.05)), 0.035, P["dark"], sides=6)
    poly([tuple(pole + Vector(p)) for p in ((1.0, 0.4, 0.02), (1.3, 0.5, 0.02), (1.25, 0.95, 0.02), (0.95, 0.85, 0.02))], mat("Team Cloth Torn", (255, 255, 255)), "flag")
    finish("ruin", stages=False)


def seal():
    """A leopard seal hauled out on the ice (head along -y): a long spotted
    body, a big reptile head with the jaw hanging open on its fangs, fore
    flippers splayed and the hind flippers fanned at the tail."""
    rng = random.Random(9)
    hide = mat("Seal Hide", (118, 124, 134))
    belly = mat("Seal Belly", (206, 210, 214))
    dark = mat("Seal Spots", (78, 84, 96))
    gum = mat("Seal Mouth", (150, 84, 96))
    teeth = mat("Gloss Teeth", (244, 242, 232))
    eye = mat("Gloss Eye", (16, 16, 20))
    ball((0, 0.18, 0.22), 1.0, belly, subdivisions=3, scale=(0.27, 0.56, 0.2))
    ball((0, 0.16, 0.3), 1.0, hide, subdivisions=3, scale=(0.26, 0.55, 0.22))
    ball((0, -0.36, 0.33), 1.0, hide, subdivisions=3, scale=(0.2, 0.26, 0.19))
    # Head: skull and upper jaw tilted up, lower jaw dropped.
    ball((0, -0.6, 0.42), 1.0, hide, subdivisions=2, scale=(0.15, 0.2, 0.11))
    obox((0, -0.74, 0.43), (0.18, 0.24, 0.07), (math.radians(-18), 0, 0), hide, bevel=0.04, segments=2, smooth=True)
    obox((0, -0.72, 0.26), (0.17, 0.26, 0.05), (math.radians(14), 0, 0), hide, bevel=0.03, segments=2, smooth=True)
    obox((0, -0.7, 0.32), (0.14, 0.22, 0.07), (0, 0, 0), gum, bevel=0.02)
    for k in range(4):
        y = -0.66 - k * 0.055
        for x in (-0.065, 0.065):
            cone((x, y, 0.39), (0, 0, -1), 0.016, 0.07 if k % 2 == 0 else 0.045, teeth, sides=4)
            cone((x, y, 0.29), (0, 0, 1), 0.014, 0.045, teeth, sides=4)
    for x in (-0.11, 0.11):
        ball((x, -0.56, 0.5), 0.03, eye, subdivisions=1)
    # Spots down the back.
    for _ in range(14):
        y = rng.uniform(-0.5, 0.65)
        x = rng.uniform(-0.16, 0.16)
        top = 0.3 + 0.22 * math.sqrt(max(0.0, 1 - (x / 0.26) ** 2 - ((y - 0.16) / 0.55) ** 2))
        ball((x, y, top - 0.01), 1.0, dark, subdivisions=1, scale=(0.05, 0.065, 0.02))
    # Fore flippers splayed on the ice, hind flippers fanned.
    for sx in (-1, 1):
        obox((sx * 0.21, -0.2, 0.1), (0.24, 0.1, 0.035), (0, sx * 0.25, sx * 0.55), hide, bevel=0.015)
        obox((sx * 0.08, 0.8, 0.1), (0.13, 0.22, 0.03), (0, 0, sx * 0.3), hide, bevel=0.012)
    ball((0, 0.7, 0.13), 1.0, hide, subdivisions=2, scale=(0.1, 0.14, 0.08))
    finish("seal", stages=False)


def power(kind):
    """A power-up on a snowy pedestal: the shield-gem (defence) or a pair of
    golden snowballs (double throw), ringed with ice spikes."""
    P = palette()
    prism((0, 0, 0), (0, 0, 1), 0.28, 0.1, P["cap"], sides=12)
    prism((0, 0, 0.1), (0, 0, 1), 0.22, 0.05, P["snow"], sides=12)
    for k in range(6):
        a = k / 6 * TAU
        cone((math.cos(a) * 0.25, math.sin(a) * 0.25, 0.1), (math.cos(a) * 0.4, math.sin(a) * 0.4, 1), 0.035, 0.14, P["ice"], sides=4)
    if kind == "defense":
        gem = mat("Glow Gloss Gem", (70, 150, 255))
        cone((0, 0, 0.5), (0, 0, 1), 0.18, 0.32, gem, sides=6)
        cone((0, 0, 0.5), (0, 0, -1), 0.18, 0.3, gem, sides=6)
        prism((0, 0, 0.47), (0, 0, 1), 0.19, 0.04, mat("Metal Silver", (200, 210, 225)), sides=6, smooth=False)
    else:
        gold = mat("Glow Gold Ball", (255, 200, 70))
        ball((-0.09, 0, 0.32), 0.14, gold, subdivisions=2)
        ball((0.09, 0, 0.56), 0.14, gold, subdivisions=2)
        for k in range(3):
            a = k / 3 * TAU
            cone((math.cos(a) * 0.05, math.sin(a) * 0.05, 0.72), (math.cos(a), math.sin(a), 1.2), 0.025, 0.12, gold, sides=4)
    finish(f"power-{kind}", stages=False)


BUILDINGS = {
    "sauna": sauna,
    "campfire": campfire,
    "store": store,
    "tower": tower,
    **{f"palisade-{i}": (lambda i=i: palisade(i)) for i in range(3)},
    **{f"igloo-{i}": (lambda i=i: igloo(i)) for i in range(3)},
    "workshop": workshop,
    "sling-range": sling_range,
    "ski-lodge": ski_lodge,
    "packer-yard": packer_yard,
    "spa": spa,
    "track": track,
    "bunkhouse": bunkhouse,
    **{f"tree-{i}": (lambda i=i: pine(i)) for i in range(3)},
    **{f"stump-{i}": (lambda i=i: stump(i)) for i in range(3)},
    **{f"crystal-{i}": (lambda i=i, s=s: crystal(s, i)) for i, s in enumerate((0.0, 0.35, 0.7, 1.0))},
    **{f"pinnacle-{i}": (lambda i=i: pinnacle(i)) for i in range(3)},
    "hole-open": lambda: fish_hole(True),
    "hole-frozen": lambda: fish_hole(False),
    "ruin": ruin,
    "seal": seal,
    "power-defense": lambda: power("defense"),
    "power-double": lambda: power("double"),
}

if __name__ == "__main__":
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    names = [a for a in args if not a.startswith("--")] or list(BUILDINGS)
    for n in names:
        reset()
        BUILDINGS[n]()
