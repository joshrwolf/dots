# Charts

Use a chart when the content is quantities. The diagram kit does not draw
charts; write them as inline SVG or plain HTML in the page, styled with the
kit's variables.

## Pick the form before the colour

Sometimes the right form is not a chart:

| The data is | Use |
|---|---|
| One current value, maybe with a trend | A stat: the number, its change, optionally a sparkline |
| A few headline numbers | A row of stats |
| One ratio against a limit | A meter (filled track) |
| More than about seven classes that all matter | A table, or a table plus a chart |

When a chart is right, the reader's job picks the type:

| Job | Form |
|---|---|
| Compare magnitudes | Bar or column; heatmap for a grid |
| Trend over time | Line; area for a single series |
| One series matters, the rest are context | Highlight that one, grey the rest |
| Above or below a baseline | Diverging bar, or a line against the baseline |
| Part of a whole | Stacked bar (horizontal for many or long names) |
| Before and after per item | Dumbbell (two dots joined) |

## Colour

- A single series is one colour for every bar, however the values vary. Bar
  length already shows magnitude; shading unordered categories darker by
  value double-encodes it and misleads.
- A one-hue light-to-dark ramp is for colour that itself encodes an ordered
  quantity (heatmap cells, ordered bins such as tiers or age bands), always
  with a scale legend.
- Distinct hues only when the series themselves are the subject: at most four
  in one chart, a legend from two series up, and direct labels on lines when
  there are up to four. Scatter plots and maps stop at three.
- Highlighting one series in the accent and greying the rest is often the
  clearest answer to "make this clearer".
- Diverging data: two hues that read as opposites (blue and red or orange)
  around a neutral grey midpoint.
- Status colours (good, warning, critical) only when the colour means status,
  always with a label or icon too.
- A series keeps its colour when others are filtered out.
- Text and labels use text colours, not the series colour, and keep at least
  4.5:1 contrast against the background (3:1 for marks and large text).

## Marks and chrome

- Thin marks: bars at most 24px thick, lines 2px with round joins, markers at
  least 8px, area fills light (about 10% of the series colour). Separate
  touching fills with a 2px gap in the background colour, never borders.
- Gridlines and axes are solid hairlines a shade off the background, never
  dashed.
- Comparable charts side by side share one scale and baseline.
- Plot time series point to point; smoothing invents values between samples.
- Label selectively: the endpoint, the extreme, the value that matters. Never
  a number on every point. A label that does not fit inside a bar goes outside
  it, never clipped.
- One y-axis per chart. Two quantities on two scales are two charts.
- Same sans typeface as the page. Tabular figures only in tables and axis
  ticks.

## Accessibility

- Every chart has a caption or heading stating its takeaway, and the SVG an
  `aria-label` saying what it plots.
- Every value stays reachable without hovering: direct labels for a few
  values, otherwise a real `<table>` of the data next to or under the chart.

## Avoid

Dual axes, rainbow scales for magnitude, pies for close values or more than
six slices, a one-bar chart (use a stat), more than seven colour classes, and
a fixed chart height that cuts off the axis labels.
