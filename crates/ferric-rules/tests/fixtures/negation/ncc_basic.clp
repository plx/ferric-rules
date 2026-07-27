; Negated conjunction: fire when it is NOT the case that both (a) and (b) exist
(deffacts startup (trigger))

(defrule no-pair
    (trigger)
    (not (and (a) (b)))
    =>
    (printout t "no a+b pair" crlf))
