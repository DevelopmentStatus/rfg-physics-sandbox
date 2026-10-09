"""Weapon archives and mesh names shared by conversion and completeness checks."""

# key: (items archive, mesh stem inside it, texture bank archive, hand)
# Mesh stems come from items_3d.xtbl (Mesh/Filename) -> *.str2_pc; texture banks hold *_d/_n/_s.
WEAPONS = {
    'sledgehammer': ('sledgehammer', 'sledgehammer', 'sledgehammer_high', 'l-handprop'),  # 'held in left hand' (E-003)
    'golden_hammer': ('golden_hammer', 'golden_hammer', 'golden_hammer_high', 'l-handprop'),
    'sledge_upgrade_2': ('sledge_upgrade_1', 'sledge_upgrade_2', 'sledge_upgrade_2_high', 'l-handprop'),
    'sledge_upgrade_3': ('sledge_upgrade_2', 'sledge_upgrade_3', 'sledge_upgrade_3_high', 'l-handprop'),
    'assault_rifle': ('m16', 'assault_rifle', 'assault_rifle_high', 'r-handprop'),
    'edf_pistol': ('edf_pistol', 'pistol', 'pistol_high', 'r-handprop'),
    'shotgun': ('shotgun', 'marauder_shotgun', 'marauder_shotgun_high', 'r-handprop'),
    'sniper_rifle': ('sniper_rifle', 'sniper_rifle', 'sniper_rifle_high', 'r-handprop'),
    'rail_driver': ('rail_driver', 'rail_driver', 'rail_driver_high', 'r-handprop'),
    'rpg_launcher': ('rpg_launcher', 'rpg', 'rpg_high', 'r-handprop'),
    'nano_rifle': ('nano_rifle', 'nanorifle', 'nanorifle_high', 'r-handprop'),
    'arc_welder': ('arc_welder', 'arc_welder', 'arc_welder_high', 'r-handprop'),
    'grinder': ('grinder', 'grinder', 'grinder_high', 'r-handprop'),
    'enforcer': ('enforcer', 'enforcer', 'Enforcer_high', 'r-handprop'),
    'gutter': ('gutter', 'marauder_gutter', 'marauder_gutter_high', 'r-handprop'),
    'gauss_rifle': ('gauss_rifle', 'gauss_rifle', 'gauss_rifle_high', 'r-handprop'),
    'autoblaster': ('autoblaster', 'autoblaster', 'autoblaster_high', 'r-handprop'),
    'remote_detonator': ('charge_placer', 'remote_detonator', 'remote_detonator_high', 'r-handprop'),
}
