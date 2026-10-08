(deffacts d (a 1))
(defrule no (not (or (b 1) (c 1))) => (printout t "no fired" crlf))
