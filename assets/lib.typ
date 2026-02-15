#let background = luma(13%)
#let foreground = rgb("#dbb877")
#let stroke-color = rgb("#977e51")

#let logo = {
  set text(
    size: 100pt,
    font: "Pixelify Sans",
    fill: foreground,
    stroke: stroke-color + 2pt,
  )
  set image(height: 90pt)

  stack(
    dir: ltr,
    spacing: .25em,
    [SETRIXTUI],
    image(
      "sand.png",
      height: 1.1em,
    ),
  )
}
