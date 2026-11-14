# Move report for `/Users/jbe390/Documents/Rust/optobench/layout-15.optobench`

Suggested by `optobench optimize`. Values are mm and degrees; moves are in the assembly (scene) frame, from the pose in the file where every DOF is 0.

## Degrees of freedom

| DOF | Unit | Current | Suggested | Change | At range limit |
| --- | --- | ---: | ---: | ---: | --- |
| PCV_slide_X | mm | 0.0000 | -3.6361 | -3.6361 | no |

## Adjustment room

How much of each adjuster's travel towards one end the suggested value uses (0 % in the middle of its range, 100 % at an end), the travel left each way, and the range to give the DOF if the move is made with the adjuster and the design exported again.

| DOF | Range | Suggested | Used | Left (−, +) | Range after adjusting | Status |
| --- | --- | ---: | ---: | --- | --- | --- |
| PCV_slide_X | -50.0000 to 50.0000 | -3.6361 | 7 % | 46.3639 to 53.6361 | -46.3639 to 53.6361 | ok |


## Moves

| Item | Translate (mm) | Rotate about | By (deg) |
| --- | --- | --- | ---: |
| element `PCV` | (-3.6361, 0.0000, 0.0000) | (0.0000, 0.0000, 1.0000) | 0.0000 |

## Merit

| Term | Detector | Unit | Before | After |
| --- | --- | --- | --- | --- |
| diameter | D1 | mm | -0.268790 | 0.043256 |
| collimation | D1 | mrad | 1.822914 | 0.008579 |

Next: Set each DOF's `value` in the layout to the suggested one.
