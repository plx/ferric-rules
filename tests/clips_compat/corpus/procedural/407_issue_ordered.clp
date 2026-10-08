(deffacts d (item 1) (item 2 3))
(defrule r => (do-for-all-facts ((?f item)) TRUE (printout t ?f:implied crlf)))
