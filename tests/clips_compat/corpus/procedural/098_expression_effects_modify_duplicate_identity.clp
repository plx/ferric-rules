(deftemplate p (slot n) (slot v))
(defrule run =>
  (bind ?f (assert (p (n a) (v 1))))
  (bind ?same (modify ?f (v 1)))
  (printout t "same:" ?same ":" (fact-index ?f) crlf)
  (bind ?changed (modify ?same (v 2)))
  (printout t "changed:" ?changed ":" (fact-index ?same) crlf)
  (printout t "duplicate:[" (duplicate ?changed) "] new:[" (duplicate ?changed (n b)) "]" crlf)
  (do-for-all-facts ((?x p)) TRUE (printout t ?x ":" ?x:n ":" ?x:v crlf)))
