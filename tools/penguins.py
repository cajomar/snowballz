"""Build every kind of penguin on the Snowballz 2 penguin: the same body,
rig and animations as the old worker and warrior, slimmed down, each kind
in its own outfit and gear, made here in Blender and pinned to the rig's
bones so it moves with the animations.

    blender -b --python tools/penguins.py               # every kind
    blender -b --python tools/penguins.py -- skier      # just one

Reads ../snowballz2/datasrc/penguin.blend (never changes it) and writes
art/units/<kind>.blend, an editable copy with the outfit on, and
assets/models/unit-<kind>.sbm, which the game draws.

Gear is written in game units (a tile is 1; the penguin stands about 1.05
tall on z = 0, faces -Y, and is centred on x = 0) as it stands in the
"Stand" pose; `on=` names the bone it rides on.
"""

import math
import os
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import export_models as em  # noqa: E402

SOURCE = "../snowballz2/datasrc/penguin.blend"
ART = "art/units"
S = em.PENGUIN_SCALE
TAU = math.tau
R = math.radians

# --------------------------------------------------------------------------
# Materials (linear colours). "Team..." takes the player's colour, "Glow..."
# stays lit, "Gloss..." shines.

PALETTE = {
    "Team Cloth": (0.9, 0.9, 0.9),
    "Team Trim": (0.9, 0.9, 0.9),
    "Wool": (0.92, 0.92, 0.88),
    "Wood": (0.32, 0.16, 0.06),
    "Wood Light": (0.6, 0.38, 0.16),
    "Leather": (0.22, 0.1, 0.04),
    "Rope": (0.62, 0.48, 0.26),
    "Gloss Metal": (0.55, 0.57, 0.62),
    "Gloss Gold": (1.0, 0.62, 0.08),
    "Gloss Ice": (0.55, 0.85, 1.0),
    "Gloss Lens": (1.0, 0.55, 0.05),
    "Glow Gem": (0.9, 0.05, 0.08),
    "Glow Puff": (1.0, 0.95, 0.85),
    "Snow": (0.95, 0.97, 1.0),
    "Red": (0.75, 0.05, 0.04),
    "Ski": (0.8, 0.08, 0.06),
    "Black": (0.02, 0.02, 0.02),
    "Olive Metal": (0.09, 0.12, 0.04),
}


def mat(name):
    m = bpy.data.materials.get(name)
    if m:
        return m
    m = bpy.data.materials.new(name)
    m.diffuse_color = (*PALETTE[name], 1.0)
    if m.node_tree:
        for n in m.node_tree.nodes:
            if n.type == "BSDF_PRINCIPLED":
                n.inputs["Base Color"].default_value = (*PALETTE[name], 1.0)
                n.inputs["Roughness"].default_value = 0.25 if name.startswith("Gloss") else 0.7
    return m


# --------------------------------------------------------------------------
# Shapes, in game units, pinned to a bone of the rig being dressed.

RIG = None
ORIGIN = Vector()


def _finish(name, bm, material, on, loc, rot, smooth=True):
    for f in bm.faces:
        f.smooth = smooth
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    ob.data.materials.append(mat(material))
    bpy.context.scene.collection.objects.link(ob)
    ob.location = loc
    ob.rotation_euler = rot
    bpy.context.view_layer.update()
    world = Matrix.Translation(ORIGIN) @ Matrix.Scale(1 / S, 4) @ ob.matrix_world
    ob.parent = RIG
    ob.parent_type = "BONE"
    ob.parent_bone = on
    bpy.context.view_layer.update()
    ob.matrix_world = world
    return ob


def sphere(name, material, radii, loc, on, rot=(0, 0, 0), seg=(10, 6), shape=None):
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=seg[0], v_segments=seg[1], radius=1.0)
    for v in bm.verts:
        x, y, z = v.co
        if shape:
            x, y, z = shape(x, y, z)
        v.co = Vector((x * radii[0], y * radii[1], z * radii[2]))
    return _finish(name, bm, material, on, loc, rot)


def cone(name, material, r1, r2, depth, loc, on, rot=(0, 0, 0), seg=10):
    """Along +Z from its base."""
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, cap_tris=False, segments=seg, radius1=r1, radius2=r2, depth=depth)
    bmesh.ops.translate(bm, verts=bm.verts, vec=(0, 0, depth / 2))
    return _finish(name, bm, material, on, loc, rot)


def box(name, material, size, loc, on, rot=(0, 0, 0), bevel=0.015, bend=None):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    for v in bm.verts:
        v.co = Vector((v.co.x * size[0], v.co.y * size[1], v.co.z * size[2]))
    if bevel > 0:
        bmesh.ops.bevel(bm, geom=bm.edges[:] + bm.verts[:], offset=bevel, segments=1, profile=0.5, affect="EDGES")
    if bend:
        for v in bm.verts:
            v.co = Vector(bend(*v.co))
    return _finish(name, bm, material, on, loc, rot)


def ring(name, material, radius, thick, loc, on, rot=(0, 0, 0), seg=(14, 5), squash=1.0):
    """A torus about Z."""
    bm = bmesh.new()
    rows = []
    for i in range(seg[0]):
        a = TAU * i / seg[0]
        row = []
        for j in range(seg[1]):
            b = TAU * j / seg[1]
            r = radius + thick * math.cos(b)
            row.append(bm.verts.new((r * math.cos(a), r * math.sin(a), thick * math.sin(b) * squash)))
        rows.append(row)
    for i in range(seg[0]):
        for j in range(seg[1]):
            n, m = (i + 1) % seg[0], (j + 1) % seg[1]
            bm.faces.new((rows[i][j], rows[n][j], rows[n][m], rows[i][m]))
    return _finish(name, bm, material, on, loc, rot)


def rod(name, material, a, b, r, on, seg=6):
    """A thin cylinder from point `a` to `b`."""
    a, b = Vector(a), Vector(b)
    d = b - a
    return cone(name, material, r, r, d.length, a, on, rot=d.to_track_quat("Z", "Y").to_euler(), seg=seg)


def hand(side):
    """The flipper tip, as it hangs in the Stand pose."""
    return (0.35 if side == "L" else -0.35, -0.02, 0.26)


HEAD = (0.0, -0.01, 0.9)  # the middle of the head
HEAD_R = 0.17


# --------------------------------------------------------------------------
# Outfits and gear


def knit_hat(material="Team Cloth", pompom=True):
    sphere("Hat", material, (0.19, 0.19, 0.14), (0, 0.0, 1.0), "spine.006", seg=(12, 6), shape=lambda x, y, z: (x, y, max(z, -0.2)))
    ring("Hat Brim", "Wool", 0.175, 0.03, (0, 0.0, 0.98), "spine.006", rot=(R(-6), 0, 0))
    if pompom:
        sphere("Pompom", "Wool", (0.055, 0.055, 0.055), (0, 0.01, 1.15), "spine.006", seg=(8, 5))


def scarf(material="Team Cloth"):
    ring("Scarf", material, 0.16, 0.04, (0, -0.01, 0.74), "spine.004", rot=(R(-8), 0, 0), squash=0.8)
    box("Scarf Tail", material, (0.07, 0.022, 0.18), (0.08, -0.17, 0.64), "spine.003", rot=(R(10), 0, R(-15)))


def snowballer():
    knit_hat()
    scarf()


def shieldbearer():
    sphere("Helmet", "Gloss Metal", (0.2, 0.2, 0.17), (0, 0.0, 0.97), "spine.006", seg=(12, 6), shape=lambda x, y, z: (x, y, max(z, -0.35)))
    box("Nose Guard", "Gloss Metal", (0.03, 0.03, 0.1), (0, -0.2, 0.9), "spine.006")
    box("Plume", "Team Cloth", (0.04, 0.2, 0.09), (0, 0.02, 1.15), "spine.006", bevel=0.02)
    ring("Gorget", "Gloss Metal", 0.17, 0.035, (0, 0.0, 0.73), "spine.004", squash=0.7)
    # The ice slab, carried on the left flipper across the front.
    box("Slab", "Gloss Ice", (0.5, 0.06, 0.62), (0.1, -0.33, 0.45), "hand.L", rot=(0, 0, R(-12)), bevel=0.04)
    ring("Slab Rim", "Team Cloth", 0.2, 0.022, (0.1, -0.365, 0.45), "hand.L", rot=(R(90), 0, R(-12)), seg=(14, 4))
    sphere("Slab Boss", "Team Cloth", (0.07, 0.025, 0.07), (0.1, -0.37, 0.45), "hand.L", seg=(8, 5))


def slinger():
    # A pointed team hood with tails flying behind.
    sphere("Hood", "Team Cloth", (0.2, 0.2, 0.18), (0, 0.01, 0.95), "spine.006", seg=(12, 6),
           shape=lambda x, y, z: (x, y + max(0.0, z) * 0.3, max(z, -0.3) if y > -0.3 else max(z, 0.2)))
    cone("Hood Point", "Team Cloth", 0.07, 0.0, 0.16, (0, 0.08, 1.1), "spine.006", rot=(R(-55), 0, 0), seg=8)
    for x in (1, -1):
        box(f"Hood Tail.{x}", "Team Cloth", (0.05, 0.26, 0.02), (0.06 * x, 0.28, 0.82), "spine.006", rot=(R(-35), 0, R(8 * x)))
    # A bandolier of spare snowballs, and the sling itself.
    rod("Bandolier", "Leather", (0.2, -0.21, 0.66), (-0.2, -0.16, 0.3), 0.025, "spine.002")
    for k in range(4):
        f = (k + 0.5) / 4
        sphere(f"Belt Ball {k}", "Snow", (0.035, 0.035, 0.035), (0.2 - 0.4 * f, -0.24 + 0.05 * f, 0.66 - 0.36 * f), "spine.002", seg=(6, 4))
    hx, hy, hz = hand("R")
    rod("Sling Cord", "Rope", (hx, hy, hz), (hx, hy, hz - 0.3), 0.01, "hand.R")
    sphere("Sling Pouch", "Leather", (0.06, 0.06, 0.04), (hx, hy, hz - 0.32), "hand.R", seg=(8, 5))
    sphere("Slung", "Snow", (0.05, 0.05, 0.05), (hx, hy, hz - 0.3), "hand.R", seg=(8, 5))


def skier():
    knit_hat()
    ring("Goggle Band", "Black", 0.175, 0.018, (0, -0.01, 0.95), "spine.006", rot=(R(-4), 0, 0))
    for side, x in (("L", 1), ("R", -1)):
        sphere(f"Goggle.{side}", "Gloss Lens", (0.06, 0.03, 0.045), (0.065 * x, -0.18, 0.96), "spine.006", seg=(8, 5))
        foot = f"foot.{side}"
        fx = 0.12 * x
        box(f"Ski.{side}", "Ski", (0.08, 0.7, 0.02), (fx, -0.12, 0.012), foot,
            bend=lambda X, Y, Z: (X, Y, Z + (0.0 if Y > -0.22 else (abs(Y) - 0.22) ** 2 * 1.6)))
        box(f"Binding.{side}", "Black", (0.09, 0.1, 0.035), (fx, -0.04, 0.035), foot)
        hx, hy, hz = hand(side)
        rod(f"Pole.{side}", "Gloss Metal", (hx, hy, hz + 0.06), (hx * 1.15, hy + 0.12, 0.0), 0.009, f"hand.{side}")
        ring(f"Basket.{side}", "Black", 0.035, 0.009, (hx * 1.14, hy + 0.11, 0.05), f"hand.{side}", seg=(10, 4))
    scarf()


def packer():
    # A team headband, a sash, goggles: it fires a snow bazooka.
    ring("Headband", "Team Cloth", 0.165, 0.03, (0, 0.0, 0.97), "spine.006")
    ring("Goggle Band", "Black", 0.172, 0.016, (0, -0.01, 0.93), "spine.006", rot=(R(-4), 0, 0))
    for x in (1, -1):
        sphere(f"Goggle.{x}", "Gloss Lens", (0.055, 0.03, 0.042), (0.065 * x, -0.18, 0.94), "spine.006", seg=(8, 5))
    rod("Sash", "Team Cloth", (0.2, -0.2, 0.7), (-0.22, -0.18, 0.3), 0.045, "spine.002")
    ring("Belt", "Team Cloth", 0.235, 0.035, (0, 0.04, 0.45), "spine.001", squash=1.2)
    # The bazooka on its right shoulder, pointing ahead: a fat tube with a
    # flared mouth, team bands, a grip and a sight.
    x, z = -0.27, 0.8
    rod("Tube", "Olive Metal", (x, 0.5, z), (x, -0.46, z), 0.095, "spine.004", seg=12)
    cone("Muzzle", "Olive Metal", 0.1, 0.14, 0.14, (x, -0.44, z), "spine.004", rot=(R(90), 0, 0), seg=12)
    ring("Muzzle Rim", "Team Trim", 0.135, 0.022, (x, -0.58, z), "spine.004", rot=(R(90), 0, 0), seg=(14, 4))
    cone("Breech", "Gloss Metal", 0.105, 0.075, 0.1, (x, 0.48, z), "spine.004", rot=(R(-90), 0, 0), seg=12)
    for k, y in enumerate((-0.2, 0.28)):
        ring(f"Band {k}", "Team Trim", 0.098, 0.025, (x, y, z), "spine.004", rot=(R(90), 0, 0), seg=(14, 4))
    box("Grip", "Leather", (0.05, 0.06, 0.18), (x, -0.12, z - 0.15), "spine.004", rot=(R(-12), 0, 0))
    box("Sight", "Black", (0.03, 0.07, 0.08), (x + 0.08, -0.26, z + 0.1), "spine.004")
    # Its shell: a big snowball in the mouth, gone once fired.
    sphere("Shell", "Snow", (0.11, 0.11, 0.11), (x, -0.56, z), "spine.004", seg=(10, 6))
    # The blast of snow out of the front and back as it fires.
    for k in range(3):
        r = 0.08 + 0.04 * k
        sphere(f"Blast {k}", "Snow", (r, r, r), (x, -0.68 - 0.15 * k, z), "spine.004", seg=(8, 5))
    sphere("Backblast", "Snow", (0.14, 0.14, 0.14), (x, 0.68, z), "spine.004", seg=(8, 5))
    # Spare shells in a rack on its back.
    box("Rack", "Wood", (0.3, 0.08, 0.28), (0, 0.3, 0.48), "spine.003", rot=(R(-8), 0, 0))
    for k, dx in enumerate((-0.09, 0.0, 0.09)):
        sphere(f"Spare {k}", "Snow", (0.06, 0.06, 0.06), (dx, 0.33, 0.66), "spine.003", seg=(8, 5))
    rod("Strap.L", "Leather", (0.12, -0.18, 0.66), (0.12, 0.26, 0.5), 0.018, "spine.002")
    rod("Strap.R", "Leather", (-0.12, -0.18, 0.66), (-0.12, 0.26, 0.5), 0.018, "spine.002")


def warmer():
    # A nurse's cap with a red cross, fluffy earmuffs, a team apron.
    box("Nurse Cap", "Wool", (0.3, 0.2, 0.15), (0, 0.0, 1.08), "spine.006", rot=(R(-14), 0, 0), bevel=0.05)
    box("Cap Cross V", "Red", (0.06, 0.02, 0.12), (0, -0.1, 1.09), "spine.006", rot=(R(-14), 0, 0), bevel=0.01)
    box("Cap Cross H", "Red", (0.12, 0.02, 0.06), (0, -0.1, 1.09), "spine.006", rot=(R(-14), 0, 0), bevel=0.01)
    ring("Muff Band", "Team Cloth", 0.19, 0.016, (0, 0.0, 0.92), "spine.006", rot=(0, R(90), 0))
    for x in (1, -1):
        sphere(f"Muff.{x}", "Wool", (0.05, 0.075, 0.075), (0.18 * x, 0.0, 0.9), "spine.006", seg=(8, 5))
    sphere("Apron", "Team Cloth", (0.24, 0.2, 0.24), (0, -0.04, 0.42), "spine.001", seg=(12, 6),
           shape=lambda x, y, z: (x, min(y, -0.5), z) if y < -0.5 else (x, -0.5, z))
    # The blow dryer in the right flipper, warm puffs coming out.
    hx, hy, hz = hand("R")
    cone("Dryer", "Red", 0.05, 0.042, 0.16, (hx, hy + 0.06, hz + 0.02), "hand.R", rot=(R(90), 0, 0))
    cone("Nozzle", "Black", 0.032, 0.025, 0.05, (hx, hy - 0.1, hz + 0.02), "hand.R", rot=(R(90), 0, 0), seg=8)
    box("Dryer Grip", "Black", (0.035, 0.04, 0.1), (hx, hy + 0.02, hz - 0.05), "hand.R")
    for k in range(3):
        r = 0.035 + 0.012 * k
        sphere(f"Puff {k}", "Glow Puff", (r, r, r), (hx, hy - 0.18 - 0.08 * k, hz + 0.02), "hand.R", seg=(6, 4))


def sled():
    # An aviator cap with ear flaps and goggles, a team harness.
    sphere("Aviator Cap", "Leather", (0.19, 0.19, 0.16), (0, 0.0, 0.96), "spine.006", seg=(12, 6), shape=lambda x, y, z: (x, y, max(z, -0.3)))
    for x in (1, -1):
        box(f"Flap.{x}", "Leather", (0.035, 0.08, 0.12), (0.17 * x, 0.0, 0.86), "spine.006", bevel=0.015)
        sphere(f"Cap Goggle.{x}", "Gloss Lens", (0.045, 0.025, 0.035), (0.06 * x, -0.15, 1.06), "spine.006", seg=(8, 5))
    ring("Harness", "Team Cloth", 0.215, 0.03, (0, 0.03, 0.58), "spine.002", rot=(R(20), 0, 0))
    # The sled, towed behind on ropes.
    def runner(X, Y, Z):
        return (X, Y, Z + (0.0 if Y > -0.2 else (abs(Y) - 0.2) ** 2 * 2.0))

    for x in (0.2, -0.2):
        box(f"Runner.{x}", "Wood", (0.04, 0.8, 0.035), (x, 0.82, 0.02), "spine", bend=runner)
        for y in (0.6, 0.82, 1.04):
            box(f"Post.{x}.{y}", "Wood", (0.03, 0.03, 0.08), (x, y, 0.08), "spine", bevel=0.006)
    for k, y in enumerate((0.56, 0.69, 0.82, 0.95, 1.08)):
        box(f"Slat {k}", "Wood Light", (0.5, 0.1, 0.025), (0, y, 0.13), "spine", bevel=0.006)
    rod("Tow Rope.L", "Rope", (0.2, 0.44, 0.08), (0.18, 0.12, 0.5), 0.01, "spine")
    rod("Tow Rope.R", "Rope", (-0.2, 0.44, 0.08), (-0.18, 0.12, 0.5), 0.01, "spine")


def chieftain():
    # A gold crown with spikes and red gems, a team cape and collar.
    ring("Crown Band", "Gloss Gold", 0.15, 0.03, (0, 0.0, 1.02), "spine.006", squash=1.6)
    for k in range(7):
        a = TAU * k / 7
        cone(f"Crown Spike {k}", "Gloss Gold", 0.035, 0.0, 0.11, (0.15 * math.sin(a), 0.15 * math.cos(a), 1.04), "spine.006", seg=6)
        if k % 2 == 0:
            sphere(f"Gem {k}", "Glow Gem", (0.02, 0.02, 0.02), (0.17 * math.sin(a), 0.17 * math.cos(a), 1.02), "spine.006", seg=(6, 4))
    sphere("Cape", "Team Cloth", (0.34, 0.07, 0.42), (0, 0.3, 0.42), "spine.003", rot=(R(8), 0, 0), seg=(12, 7),
           shape=lambda x, y, z: (x * (1.0 + 0.3 * max(0.0, -z)), y, z))
    ring("Collar", "Team Trim", 0.17, 0.04, (0, 0.0, 0.74), "spine.004", squash=0.8)
    sphere("Clasp", "Gloss Gold", (0.04, 0.025, 0.04), (0, -0.18, 0.72), "spine.004", seg=(8, 5))


# --------------------------------------------------------------------------
# Poses made in code, laid over the source's own animations. Each gets the
# rig and the frame (0 to 5).


def _rot(rig, bone, axis, deg):
    pb = rig.pose.bones[bone]
    pb.rotation_quaternion = pb.rotation_quaternion @ Quaternion(axis, R(deg))


def lean(deg):
    def pose(rig, f):
        _rot(rig, "spine.001", (1, 0, 0), deg)
    return pose


def knocked(rig, f):
    """Flat on its back, flippers and feet waving."""
    s = math.sin(f / 6 * TAU)
    rig.rotation_euler = Euler((R(-82), 0, R(4) * s))
    rig.location = ORIGIN + Vector((0, 0.15 / S, 0.32 / S))
    _rot(rig, "upper_arm.L", (0, 0, 1), 70 + 25 * s)
    _rot(rig, "upper_arm.R", (0, 0, 1), -70 + 25 * s)
    _rot(rig, "spine.006", (1, 0, 0), 20)
    for side, k in (("L", 1), ("R", -1)):
        _rot(rig, f"thigh.{side}", (1, 0, 0), -25 + 20 * s * k)


def warm(rig, f):
    s = math.sin(f / 6 * TAU * 2)
    _rot(rig, "upper_arm.R", (1, 0, 0), 75 + 5 * s)
    _rot(rig, "upper_arm.R", (0, 0, 1), -25)
    _rot(rig, "spine.001", (1, 0, 0), 6)


def brace(rig, f):
    """Flippers up on the bazooka, then the kick of the shot."""
    _rot(rig, "upper_arm.R", (0, 0, 1), -60)
    _rot(rig, "upper_arm.R", (1, 0, 0), 40)
    _rot(rig, "upper_arm.L", (0, 0, 1), 35)
    _rot(rig, "upper_arm.L", (1, 0, 0), 70)
    kick = [0, 0, 0, -14, -8, -3][f]
    _rot(rig, "spine.001", (1, 0, 0), 6 + kick)


def hold(rig, f):
    """Flippers resting on the bazooka."""
    _rot(rig, "upper_arm.R", (0, 0, 1), -60)
    _rot(rig, "upper_arm.R", (1, 0, 0), 40)
    _rot(rig, "upper_arm.L", (0, 0, 1), 35)
    _rot(rig, "upper_arm.L", (1, 0, 0), 70)


def cheer(rig, f):
    em.pose_cheer(rig, f)


def flinch(rig, f):
    em.pose_flinch(rig, f)


# Every clip: (source tracks, procedural pose or None, objects shown only
# then with the frames they show in, or None for every frame).
FIGHTER = {
    "stand": (["Stand"], None, {}),
    "walk": (["Walk"], None, {}),
    "throw": (["Throw"], None, {"Snowball.001": None}),
    "scoop": (["Bend"], None, {"Snowball": None}),
    "cheer": (["Stand"], cheer, {}),
    "flinch": (["Stand"], flinch, {}),
    "knocked": (["Stand"], knocked, {}),
}


def clips_for(kind):
    c = {k: (t, p, dict(sh)) for k, (t, p, sh) in FIGHTER.items()}
    if kind == "skier":
        c["walk"] = (["Walk"], lean(18), {})
    if kind == "sled":
        c["walk"] = (["Walk"], lean(14), {})
        del c["throw"], c["scoop"]
    if kind == "packer":
        shell = {"Shell": (0, 1, 2)}
        c["throw"] = (["Stand"], brace, {**shell, "Blast 0": (3, 4), "Blast 1": (3, 4), "Blast 2": (4,), "Backblast": (3, 4)})
        c["stand"] = (["Stand"], hold, dict(shell))
        for k in ("walk", "scoop", "cheer", "flinch", "knocked"):
            c[k] = (c[k][0], c[k][1], dict(shell))
    if kind == "warmer":
        del c["throw"]
        c["warm"] = (["Stand"], warm, {"Puff 0": None, "Puff 1": None, "Puff 2": None})
    if kind == "slinger":
        c["throw"] = (["Throw"], None, {"Slung": (0, 1, 2)})
    return c


# The old worker, as before, plus being knocked flat.
WORKER = {
    "stand": (["Stand"], None, {}),
    "walk": (["Walk"], None, {}),
    "carry": (["Walk", "Carry"], None, {"CarryFish": None}),
    "fish": (["Stand", "Chop"], None, {"Fishing Pole": None}),
    "unload": (["Bend", "Unload Tree"], None, {"UnloadFish": None}),
    "hammer": (["Stand", "Hammer"], None, {"Hammer Handle": None, "Hammer Head": None}),
    "chop": (["Stand", "Harvest Tree"], None, {"Axe Handle": None, "Cube": None}),
    "sit": (["Sit"], None, {}),
    "cheer": (["Stand"], cheer, {}),
    "flinch": (["Stand"], flinch, {}),
    "knocked": (["Stand"], knocked, {}),
}

KINDS = {
    # name: (base penguin, its own outfit from the source, gear)
    "worker": ("worker", ["Stocking Cap.001"], None),
    "snowballer": ("warrior", ["Toga Mesh"], snowballer),
    "shieldbearer": ("warrior", [], shieldbearer),
    "slinger": ("warrior", [], slinger),
    "skier": ("warrior", [], skier),
    "packer": ("warrior", [], packer),
    "warmer": ("warrior", [], warmer),
    "sled": ("warrior", [], sled),
    "chieftain": ("warrior", ["Toga Mesh"], chieftain),
}

RIGS = {"worker": "Worker Metarig", "warrior": "Warrior Metarig"}
OUTFIT_RIGS = {"worker": "Stocking Cap", "warrior": "Toga"}
BODIES = {"worker": "Meshes", "warrior": "Warrior Penguin Meshes"}

# Slimming the source penguin: what each part keeps after decimating.
SLIM = {"Foot": 0.6, "Toga": 0.6, "Stocking Cap": 0.6}
# The body is subdivided once first (its cage alone is ridged and faceted
# at the flippers), then thinned to this share of the smooth surface.
BODY_KEEP = 0.4


def slim(scene):
    """The source penguin with its accessories trimmed (as the old export
    did), and then the body, feet and outfits thinned out too: a few
    hundred of them can be on screen at once."""
    em.simplify(scene)
    em.lighten(scene)
    for o in scene.objects:
        if o.type != "MESH":
            continue
        if o.name.startswith("Body"):
            smooth_body(o)
            continue
        for prefix, ratio in SLIM.items():
            if o.name.startswith(prefix):
                # First in the stack, so every frame has the same triangles.
                d = o.modifiers.new("slim", "DECIMATE")
                d.ratio = ratio
                while o.modifiers[0] != d:
                    o.modifiers.move(len(o.modifiers) - 1, 0)


def smooth_body(o):
    """Subdivide, then decimate, ahead of the rig, so every frame has the
    same triangles and the flippers come out round."""
    for m in [m for m in o.modifiers if m.type in ("SUBSURF", "DECIMATE")]:
        o.modifiers.remove(m)
    sub = o.modifiers.new("smooth", "SUBSURF")
    sub.levels = sub.render_levels = 1
    d = o.modifiers.new("slim", "DECIMATE")
    d.ratio = BODY_KEEP
    for k, m in enumerate((sub, d)):
        while o.modifiers[k] != m:
            o.modifiers.move(o.modifiers.find(m.name), k)
    for p in o.data.polygons:
        p.use_smooth = True


def reset(rig):
    rig.rotation_euler = Euler((0, 0, 0))
    rig.location = ORIGIN
    for pb in rig.pose.bones:
        pb.rotation_quaternion = Quaternion()
        pb.location = Vector()
        pb.scale = Vector((1, 1, 1))


def build(kind):
    global RIG, ORIGIN
    base, outfit, gear = KINDS[kind]
    bpy.ops.wm.open_mainfile(filepath=os.path.abspath(SOURCE))
    scene = bpy.context.scene
    em.show_everything()
    slim(scene)
    em.setup_fish_props(scene)
    em.add_action_track(scene.objects["Worker Metarig"], "Harvest Tree")
    em.thicken_tools(scene)
    RIG = scene.objects[RIGS[base]]
    ORIGIN = RIG.matrix_world.translation.copy()
    em.set_pose(RIG, ["Stand"])
    scene.frame_set(0)
    bpy.context.view_layer.update()
    made_before = set(scene.objects.keys())
    if gear:
        gear()
    made = [n for n in scene.objects.keys() if n not in made_before]
    clips = WORKER if kind == "worker" else clips_for(kind)
    shown_somewhere = {n for _, _, sh in clips.values() for n in sh}
    body = [o.name for o in bpy.data.collections[BODIES[base]].objects]
    always = body + outfit + [n for n in made if n not in shown_somewhere]
    sizes = {n: scene.objects[n].scale.copy() for n in shown_somewhere}
    encoded = []
    for clip, (tracks, pose, shown) in clips.items():
        em.set_pose(RIG, tracks)
        orig = scene.objects.get(OUTFIT_RIGS[base])
        if orig:
            em.outfit_action(orig, tracks)
        objs = [scene.objects[n] for n in always + list(shown)]
        baker = em.Baker(objs, ORIGIN, S)
        for f in range(6):
            reset(RIG)
            scene.frame_set(f)
            if pose:
                pose(RIG, f)
            for n, frames in shown.items():
                ob = scene.objects[n]
                ob.scale = sizes[n] if frames is None or f in frames else sizes[n] * 0.001
            bpy.context.view_layer.update()
            baker.capture()
        encoded.append(baker.encode(clip))
    reset(RIG)
    scene.frame_set(0)
    os.makedirs(ART, exist_ok=True)
    em.write_model(f"{em.OUT_DIR}/unit-{kind}.sbm", encoded)
    # An editable copy with the outfit on (the source stays as it was).
    bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(f"{ART}/{kind}.blend"), copy=True)


if __name__ == "__main__":
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    for kind in args or list(KINDS):
        build(kind)
