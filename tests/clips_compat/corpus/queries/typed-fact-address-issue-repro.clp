(deftemplate item (slot v))
(deffacts seed (item (v 1)) (item (v 2)))
(defrule r ?f <- (item (v 1)) =>
  (printout t ?f " " (integerp ?f) " " (numberp ?f) crlf)
  (printout t (find-all-facts ((?g item)) TRUE) crlf)
  (printout t (fact-relation 4294967299) crlf)
  (do-for-fact ((?g item)) (= ?g:v 2) (printout t (+ ?g 0) crlf)))
