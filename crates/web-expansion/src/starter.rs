//! The map **+ New map** adds: a very small place that is already a place.
//!
//! Gentle sculpted ground, a tree, a stump of rock and a barrel, a start
//! point in the open. It validates, it plays, and a new modder has something
//! to look at and something to move. All of it is coordinates and numbers:
//! the ground's texture cells and the props' meshes are named on the disc
//! (`town01`, Rim Elm), so the template holds no disc bytes. The meshes are
//! the [mesh catalog](legaia_expansion_format::mesh_catalog)'s labels.

use serde_json::{Value, json};

/// The scene the starter map is laid over: every disc has Rim Elm.
pub const STARTER_SCENE: &str = "town01";

/// The starter map, as the JSON form of `MapDef` (the pack's own spelling).
pub fn starter_map(name: &str) -> Value {
    const W: usize = 14;
    const H: usize = 10;
    let rows: Vec<String> = (0..H).map(|_| ".".repeat(W)).collect();
    // Rim Elm's short turf: nine cells of one palette, so the ground does not
    // read as a repeating pattern.
    let turf: Vec<Value> = [24, 25, 26, 32, 33, 34, 40, 41, 42]
        .map(|tile| json!({ "tpage": 12, "clut": 32000, "tile": tile }))
        .to_vec();
    json!({
        "name": name,
        "size": null,
        "base": { "scene": { "kind": "scene", "name": STARTER_SCENE }, "geometry": "none" },
        "floor_heights": [0, 64],
        "tiles": {
            "origin": [16, 16],
            "rows": rows,
            "legend": {
                ".": {
                    "label": "grass",
                    "height": 0,
                    "ground": { "kind": "texture", "tpage": 12, "clut": 32000, "tile": 24 },
                },
            },
        },
        "terrain": {
            "resolution": 4,
            "smooth": 2,
            "max_step": 44,
            "skirt": 3,
            "seed": 7,
            "variation": 0.2,
            "sun": [150.0, 42.0],
            "shading": 0.6,
            "shapes": [
                { "kind": "raise", "at": [3.0, 2.5], "radius": 4.5, "height": 26 },
                { "kind": "raise", "at": [11.5, 7.5], "radius": 4.0, "height": 18 },
                { "kind": "noise", "amplitude": 4, "scale": 2.5, "seed": 3 },
                { "kind": "flatten", "at": [7.0, 5.0], "radius": 2.5 },
            ],
            "materials": [
                {
                    "name": "grass",
                    "chars": ".",
                    "tint": [130, 136, 120],
                    "cells": turf,
                },
            ],
        },
        "props": [
            { "id": "tree", "tile": [3, 3], "mesh": { "object": 75 }, "solid": true },
            { "id": "mound", "tile": [10, 2], "mesh": { "object": 39 }, "solid": true },
            { "id": "barrel", "tile": [9, 6], "mesh": { "object": 33 }, "rotation": [0.0, 30.0, 0.0], "solid": true },
        ],
        "npcs": [],
        "spawns": [{ "id": "start", "tile": [7, 5], "facing": 0 }],
        "exits": [],
        "triggers": [],
        "encounters": [],
        "bgm": null,
        "camera": "default",
    })
}
