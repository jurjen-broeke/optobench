# Move report for `/Users/jbe390/Documents/Rust/optobench/layout-14.optobench`

Suggested by `optobench optimize`. Values are mm and degrees; moves are in the assembly (scene) frame, from the pose in the file where every DOF is 0.

## Degrees of freedom

| DOF | Unit | Current | Suggested | Change | At range limit |
| --- | --- | ---: | ---: | ---: | --- |
| PCV_slide_x | mm | 5.0000 | 0.0004 | -4.9996 | no |
| PCV_slide_y | mm | 5.0000 | 0.0003 | -4.9997 | no |

## Adjustment room

How much of each adjuster's travel towards one end the suggested value uses (0 % in the middle of its range, 100 % at an end), the travel left each way, and the range to give the DOF if the move is made with the adjuster and the design exported again.

| DOF | Range | Suggested | Used | Left (−, +) | Range after adjusting | Status |
| --- | --- | ---: | ---: | --- | --- | --- |
| PCV_slide_x | -5.0000 to 5.0000 | 0.0004 | 0 % | 5.0004 to 4.9996 | -5.0004 to 4.9996 | ok |
| PCV_slide_y | -5.0000 to 5.0000 | 0.0003 | 0 % | 5.0003 to 4.9997 | -5.0003 to 4.9997 | ok |


## Moves

| Item | Translate (mm) | Rotate about | By (deg) |
| --- | --- | --- | ---: |
| element `PCV` | (0.0000, 0.0004, 0.0003) | (0.0000, 0.0000, 1.0000) | 0.0000 |

## Merit

| Term | Detector | Unit | Before | After |
| --- | --- | --- | --- | --- |
| collimation | D1 | mrad | 19.454311 | 15.371066 |
| diameter | D1 | mm | -2.361846 | -1.184187 |

Next: Set each DOF's `value` in the layout to the suggested one.
