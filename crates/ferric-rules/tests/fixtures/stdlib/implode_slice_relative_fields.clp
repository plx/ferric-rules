
(defrule probe =>
(printout t "[" (implode$ (subseq$ (create$ before a "two words" 3 after) 2 4)) "]" crlf)
(printout t "[" (implode$ (subseq$ (create$ before "" after) 2 2)) "]" crlf)
(printout t "[" (implode$ (subseq$ (create$ a) 2 1)) "]" crlf)
)
