// A short deck with speaker notes, for the README screenshots (`just screenshots`).
#import "@preview/typslides:1.3.4": *

#show: typslides.with(
  ratio: "16-9",
  theme: "bluey",
  font: "Aptos",
  font-size: 20pt,
)

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

#front-slide(
  title: "Why the sea rises twice a day",
  subtitle: [A demo deck for `ppm`],
  authors: "ppm",
)

#slide(title: "Most coasts see two high tides a day")[
  - The water rises for about 6 hours, then falls for about 6.
  - The cycle repeats every 12 hours and 25 minutes.
  - So high tide comes about 50 minutes later each day.
  #note("Open with the question the audience already has: **why twice**, if the Moon passes over once a day?

Ask for a show of hands: who has watched a tide come in?")
]

#slide(title: "The Moon pulls the near side hardest")[
  #cols(columns: (1fr, 1fr))[
    #framed(title: "Near side")[Pulled toward the Moon more than Earth's center is.]
  ][
    #framed(title: "Far side")[Pulled less than Earth's center, so it lags behind.]
  ]
  #note("The key idea is the **difference** in pull across the Earth, not the pull itself.

- Near side: stretched toward the Moon
- Far side: left behind
- Both make a bulge")
]

#focus-slide[
  Two bulges, so two high tides.
  #note("Pause here. Let the room connect the two bulges to the two daily tides before moving on.")
]

#slide(title: "The Sun adds spring and neap tides")[
  - New and full Moon: Sun and Moon line up, so the tides are largest.
  - Quarter Moon: they pull at right angles, so the tides are smallest.
  #note("The Sun's tidal pull is a bit under half the Moon's.

Transition: *if the Sun matters, what sets the timing on a real coast?*")
]

#slide(title: "Take-home")[
  - Tides come from the difference in the Moon's pull across the Earth.
  - That difference raises two bulges, so most coasts get two tides a day.
  #note("Restate both points in one breath, then thank the audience.")
]
