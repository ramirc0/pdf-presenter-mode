// Four slides with no page fill, so renders must be white at the corners.
#import "@preview/typslides:1.3.4": *
#show: typslides.with(ratio: "16-9", theme: "bluey")

#front-slide(title: "Fixture deck", subtitle: [For `ppm` tests], authors: "ppm")

#slide(title: "Text")[
  - A bullet with #stress[stressed] text.
  - A second bullet.
]

#slide(title: "Columns")[
  #cols(columns: (1fr, 1fr))[Left column.][Right column.]
]

#blank-slide[
  #framed(title: "Framed")[A framed block on a blank slide.]
]
