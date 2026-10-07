(deftemplate p (slot n) (slot v))
(defrule r1 =>
  (bind ?f (assert (p (n a) (v 1))))
  (if (assert (p (n b) (v 2))) then (printout t "if ok" crlf))
  (bind ?g (modify ?f (v 10)))
  (bind ?h (duplicate ?g (n c)))
  (printout t (fact-index ?g) " " (fact-index ?h) crlf)
  (do-for-all-facts ((?x p)) TRUE (printout t ?x ":" ?x:n ":" ?x:v crlf)))
