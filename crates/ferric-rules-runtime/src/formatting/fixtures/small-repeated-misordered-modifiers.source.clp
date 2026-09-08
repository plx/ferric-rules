(defrule probe =>
(printout t "a:[" (format nil "%--5d" 7) "]" crlf)
(printout t "b:[" (format nil "%0-05d" 7) "]" crlf)
(printout t "c:[" (format nil "%5-3d" 7) "]" crlf)
(printout t "d:[" (format nil "%.2.3d" 7) "]" crlf)
(printout t "e:[" (format nil "%..d" 7) "]" crlf)
(printout t "f:[" (format nil "%.-3d" 7) "]" crlf)
(printout t "g:[" (format nil "%1.2-3d" 7) "]" crlf)
)
