(deffacts d (go 2))
(defrule r (go ?x) (test (if (> ?x 0) then TRUE else FALSE)) => (printout t fired crlf))
