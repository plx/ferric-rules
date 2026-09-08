
(defrule probe =>
(printout t "[" (implode$ (create$ (sin 1.0e309) 1.0e309 -1.0e309)) "]" crlf)
)
