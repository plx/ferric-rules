
(defrule exercise =>
(printout t (create$ (sin 1.0e309) 1.0e309 -1.0e309) crlf)
(printout t (sin 1.0e309) "|" 1.0e309 "|" -1.0e309 crlf)
)
