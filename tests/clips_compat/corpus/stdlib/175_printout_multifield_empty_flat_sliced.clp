
(defrule exercise =>
(printout t (create$) "|" (create$ "") crlf)
(printout t (create$ (create$ "a" "two words") (create$) tail) crlf)
(printout t (subseq$ (create$ discarded "a" "two words" tail) 2 3) crlf)
)
