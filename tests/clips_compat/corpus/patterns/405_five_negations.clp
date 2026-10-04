(deffacts d )
(defrule r (not (not (not (not (not (a)))))) => (printout t "five_negations" crlf))
