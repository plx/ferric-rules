(deffacts d (a 1))
(defrule r (not (not (or (a 1) (b 1)))) => (printout t present crlf))
