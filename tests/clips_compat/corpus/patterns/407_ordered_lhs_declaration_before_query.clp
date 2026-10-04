(defrule r (item ?x) (test (any-factp ((?f item)) TRUE)) => (printout t ?x crlf))
(deffacts d (item 7))
