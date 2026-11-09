// Speaker notes through the spec's snippet. Each note names the page it must land on.
#import "@preview/typslides:1.3.4": *
#show: typslides.with(ratio: "16-9", theme: "bluey")

#let note(body) = {
  assert(type(body) == str, message: "note takes a string")
  [#metadata(body) <speaker-note>]
}
#show: deck => {
  deck
  context pdf.attach(
    "notes.pdfpc",
    bytes(json.encode((
      pdfpcFormat: 2,
      pages: query(<speaker-note>).map(n => (
        idx: n.location().page() - 1,
        label: str(n.location().page()),
        overlay: 0,
        note: n.value,
      )),
    ))),
    mime-type: "application/json",
  )
}

#front-slide(title: "Notes fixture", subtitle: [For `ppm` tests], authors: "ppm")

#slide(title: "Two notes")[
  - First point.
  #note("Page 2, first note.")
  - Second point.
  #note("Page 2, second note.")
]

#slide(title: "Overflow")[
  #note("Page 3, before the break.")
  #for i in range(1, 21) [- Bullet #i.]
  #note("Page 4, after the break.")
]

#slide(title: "Columns")[
  #cols(columns: (1fr, 1fr))[Left column. #note("Page 5, inside cols.")][Right column.]
]

#blank-slide[
  #framed(title: "Framed")[A framed block. #note("Page 6, inside framed.")]
]

#focus-slide[
  Focus. #note("Page 7, inside focus-slide.")
]

#slide(title: "Markdown")[
  - Rendered notes.
  #note("Page 8 has **bold**, `code` and a list:\n\n- one\n- two")
]

#slide(title: "No notes")[
  - This page has no note.
]
