# Move report for `/Users/jbe390/Documents/Rust/optobench/layout-14.optobench`

Suggested by `optobench optimize`. Values are mm and degrees; moves are in the assembly (scene) frame, from the pose in the file where every DOF is 0.

## Degrees of freedom

| DOF | Unit | Current | Suggested | Change | At range limit |
| --- | --- | ---: | ---: | ---: | --- |
| PCC_slide_x | mm | 0.0000 | -0.6397 | -0.6397 | no |
| PCV_slide_x | mm | 0.0000 | 1.2794 | 1.2794 | no |

## Adjustment room

How much of each adjuster's travel towards one end the suggested value uses (0 % in the middle of its range, 100 % at an end), the travel left each way, and the range to give the DOF if the move is made with the adjuster and the design exported again.

| DOF | Range | Suggested | Used | Left (−, +) | Range after adjusting | Status |
| --- | --- | ---: | ---: | --- | --- | --- |
| PCC_slide_x | -5.0000 to 5.0000 | -0.6397 | 13 % | 4.3603 to 5.6397 | -4.3603 to 5.6397 | ok |
| PCV_slide_x | -5.0000 to 5.0000 | 1.2794 | 26 % | 6.2794 to 3.7206 | -6.2794 to 3.7206 | ok |


## Moves

| Item | Translate (mm) | Rotate about | By (deg) |
| --- | --- | --- | ---: |
| element `PCC` | (0.0000, -0.6397, 0.0000) | (0.0000, 0.0000, 1.0000) | 0.0000 |
| element `PCV` | (0.0000, 1.2794, 0.0000) | (0.0000, 0.0000, 1.0000) | 0.0000 |

## Merit

| Term | Detector | Unit | Before | After |
| --- | --- | --- | --- | --- |
| collimation | D1 | mrad | 11.363545 | 10.047782 |
| diameter | D1 | mm | 3.142519 | 2.180000 |

Next: Set each DOF's `value` in the layout to the suggested one.
