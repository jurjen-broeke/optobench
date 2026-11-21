//! Groups (M16): items that move together. A group's DOF moves every member that carries
//! DOFs by the same amount (the first leads, the others follow one to one).

use optobench_commands::Registry;
use serde_json::{Value, json};

const LAYOUT: &str = "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
    [[source]]\nid = \"laser\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\ndiameter = 1.0\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
    [[element]]\nid = \"L1\"\nkind = \"ideal_lens\"\nfocal_length = -12\nhole = [2, 0]\nheight = 75.0\naxis = [1, 0, 0]\ngroup = \"expander\"\n\n\
    [[element]]\nid = \"L2\"\nkind = \"ideal_lens\"\nfocal_length = 60\nhole = [4, 0]\nheight = 75.0\naxis = [1, 0, 0]\ngroup = \"expander\"\n\n\
    [[proxy]]\nid = \"iris\"\nkind = \"plate\"\nhole_diameter = 10.0\nsize = 30.0\nhole = [5, 0]\nheight = 75.0\naxis = [1, 0, 0]\ngroup = \"expander\"\n\n\
    [[detector]]\nid = \"out\"\nhole = [8, 0]\nheight = 75.0\naxis = [1, 0, 0]\nsize = [20, 20]\npixels = [20, 20]\n";

fn run(command: &str, text: &str, label: &str, extra: Value) -> Result<Value, String> {
    let path = std::env::temp_dir().join(format!(
        "optobench-groups-{label}-{}.optobench",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("written");
    let mut p = extra;
    p["path"] = json!(path.to_string_lossy());
    let result = Registry::standard().run(command, &p).map_err(|e| e.message);
    let _ = std::fs::remove_file(&path);
    result
}

fn origin(view: &Value, id: &str) -> [f64; 3] {
    let item = view["items"]
        .as_array()
        .and_then(|l| l.iter().find(|i| i["id"] == id))
        .unwrap_or_else(|| panic!("{id}"));
    [0, 1, 2].map(|k| item["origin"][k].as_f64().expect("placed"))
}

#[test]
fn a_groups_dof_moves_its_members_together() {
    let text = format!(
        "{LAYOUT}\n[[group]]\nid = \"expander\"\n\
         dof = [ {{ name = \"expander_y\", kind = \"slide\", axis = \"y\", range = [-5, 5] }},\n\
                 {{ name = \"expander_tip\", kind = \"tilt\", axis = \"z\", pivot = [75, 0, 75], range = [-90, 90] }} ]\n"
    );
    let still = run("view", &text, "still", json!({})).expect("viewed");
    // The file's own entries keep `group`; the DOFs are the leader's and its follower's.
    assert_eq!(still["design"]["element"][0]["group"], "expander");
    let names: Vec<&str> = still["dofs"]
        .as_array()
        .expect("dofs")
        .iter()
        .filter_map(|d| d["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "expander_y",
            "expander_tip",
            "expander_y_L2",
            "expander_tip_L2"
        ],
        "the proxy carries none"
    );
    // 3 mm along y: both lenses move 3 mm, the detector and source stay.
    let moved = run(
        "view",
        &text,
        "moved",
        json!({ "dofs": { "expander_y": 3.0 } }),
    )
    .expect("viewed");
    for id in ["L1", "L2"] {
        let (a, b) = (origin(&still, id), origin(&moved, id));
        assert!(
            (b[1] - a[1] - 3.0).abs() < 1e-9 && (b[0] - a[0]).abs() < 1e-9,
            "{id}"
        );
    }
    assert_eq!(origin(&still, "out"), origin(&moved, "out"));
    // 90° about z through (75, 0, 75): L1 at x 50 goes to y −25, L2 at x 100 to y 25,
    // their axes from +x to +y.
    let turned = run(
        "view",
        &text,
        "turned",
        json!({ "dofs": { "expander_tip": 90.0 } }),
    )
    .expect("viewed");
    let (l1, l2) = (origin(&turned, "L1"), origin(&turned, "L2"));
    assert!(
        (l1[0] - 75.0).abs() < 1e-9 && (l1[1] + 25.0).abs() < 1e-9,
        "{l1:?}"
    );
    assert!(
        (l2[0] - 75.0).abs() < 1e-9 && (l2[1] - 25.0).abs() < 1e-9,
        "{l2:?}"
    );
}

#[test]
fn group_mistakes_are_refused() {
    for (group, words) in [
        (
            "[[group]]\nid = \"expander\"\ndof = [ { name = \"g\", kind = \"tilt\", axis = \"z\" } ]\n",
            "give its `pivot`",
        ),
        (
            "[[group]]\nid = \"expander\"\ndof = [ { name = \"g\", kind = \"slide\", axis = \"x\", frame = \"local\" } ]\n",
            "scene's frame",
        ),
        (
            "[[group]]\nid = \"lonely\"\ndof = [ { name = \"g\", kind = \"slide\", axis = \"x\" } ]\n",
            "no element, source or detector",
        ),
        (
            "[[group]]\nid = \"expander\"\nmembers = [\"L1\"]\n",
            "members",
        ),
        (
            "[[group]]\nid = \"expander\"\noccurrences = [\"mount:1\"]\n",
            "only a sidecar has",
        ),
    ] {
        let e = run("trace", &format!("{LAYOUT}\n{group}"), "bad", json!({})).expect_err(words);
        assert!(e.contains(words), "{words}: {e}");
    }
}
