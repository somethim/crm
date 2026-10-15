// The thermal receipt (task 012). Rust passes one input, `data`: a JSON string with the
// page settings (width_mm, font_pt) and the pre-formatted receipt fields. Every value is
// inserted as a string, never evaluated as markup.

#let d = json(bytes(sys.inputs.data))
#let body = d.font_pt * 1pt
#let big = calc.max(11, d.font_pt + 2) * 1pt
#let rule = line(length: 100%, stroke: 0.75pt)
// Text that wraps at spaces; a word wider than the available width may also break inside
// (like the old receipt's overflow-wrap: anywhere), so it never runs into the next column.
#let fit(s) = layout(size => s
  .split(" ")
  .map(w => if measure(w).width > size.width { w.clusters().join(sym.zws) } else { w })
  .join(" "))

#set page(width: d.width_mm * 1mm, height: auto, margin: 1mm, fill: white)
// Line height 1.12: the line box spans ascender..descender (about 1.07 em for Noto Sans),
// plus a small leading.
#set text(font: "Noto Sans", size: body, top-edge: "ascender", bottom-edge: "descender")
#set par(leading: 0.05em, spacing: 0.5mm, justify: false)

#align(center, text(weight: "bold")[Porosia Nr: #d.nr])
#v(1.5mm, weak: true)
Data: #d.issued_on

#fit(d.owner_label + ": " + d.owner_name)

#if d.description != "" { fit("Përshkrimi: " + d.description) }

#v(1mm, weak: true)
#rule
#v(0.5mm, weak: true)

#grid(
  columns: (7fr, 48fr, 15fr, 15fr, 15fr),
  column-gutter: 1mm,
  row-gutter: 1.5mm,
  align: (left, left, right, right, right),
  [\#], [Artikull], [Sasia], [Çmimi], [Vlera],
  grid.cell(colspan: 5, rule),
  ..d.lines.map(l => (l.idx, fit(l.name), l.qty, l.price, l.value)).flatten(),
  grid.cell(colspan: 5, rule),
  grid.cell(colspan: 2, text(size: big, weight: "bold")[Totali]),
  [], [],
  text(size: big, weight: "bold", d.total),
)
