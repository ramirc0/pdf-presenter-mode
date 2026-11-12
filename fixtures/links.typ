// Three slides for the link tests. The first links to the others, by label and by position,
// and to the web.
#set page(width: 16cm, height: 9cm, margin: 1cm)
#set text(size: 14pt)

#link(<end>)[To the last slide] \
#link((page: 2, x: 0pt, y: 0pt))[To the second slide] \
#link("https://typst.app")[A web page] \
#link("www.example.com")[A bare address] \
#link("file:///etc/hostname")[A local file]

#pagebreak()
The second slide.

#pagebreak()
= The end <end>
