# Move report for `/Users/jbe390/Documents/Rust/optobench/layout-16.optobench`

Suggested by `optobench optimize`. Values are mm and degrees; moves are in the assembly (scene) frame, from the pose in the file where every DOF is 0.

## Degrees of freedom

| DOF | Unit | Current | Suggested | Change | At range limit |
| --- | --- | ---: | ---: | ---: | --- |
| PCV_slide_X | mm | 0.0000 | -153.6146 | -153.6146 | no |

## Adjustment room

How much of each adjuster's travel towards one end the suggested value uses (0 % in the middle of its range, 100 % at an end), the travel left each way, and the range to give the DOF if the move is made with the adjuster and the design exported again.

| DOF | Range | Suggested | Used | Left (−, +) | Range after adjusting | Status |
| --- | --- | ---: | ---: | --- | --- | --- |
| PCV_slide_X | -350.0000 to 50.0000 | -153.6146 | 2 % | 196.3854 to 203.6146 | -196.3854 to 203.6146 | ok |


## Moves

| Item | Translate (mm) | Rotate about | By (deg) |
| --- | --- | --- | ---: |
| element `PCV` | (-153.6146, 0.0000, 0.0000) | (0.0000, 0.0000, 1.0000) | 0.0000 |

## Merit

| Term | Detector | Unit | Before | After |
| --- | --- | --- | --- | --- |
| collimation | D1 | mrad | 80.625800 | 0.018925 |
| diameter | D1 | mm | -0.960794 | 0.034725 |

Next: Set each DOF's `value` in the layout to the suggested one.
