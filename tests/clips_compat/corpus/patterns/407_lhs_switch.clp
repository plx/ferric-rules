(deffacts d (go 2))
(defrule r (go ?x) (test (switch ?x (case 2 then TRUE) (default FALSE))) => (printout t fired crlf))
