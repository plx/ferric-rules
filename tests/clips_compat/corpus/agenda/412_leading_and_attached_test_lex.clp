(deffacts seed (u 3))
(defrule leading (test (> 1 0)) (u ?x) => (printout t LEADING crlf))
(defrule attached (u ?x) (test (> ?x 0)) => (printout t ATTACHED crlf))
