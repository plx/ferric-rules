(deftemplate p (slot v))
(deffacts seed (p (v 1)) (p (v 2)))
(defrule run =>
  (bind ?n 0)
  (delayed-do-for-all-facts ((?x p)) TRUE
    (bind ?n (+ ?n 1))
    (printout t "before:" ?n ":" ?x ":" ?x:v crlf)
    (if (= ?n 1) then (reset))
    (printout t "after:" ?n ":" ?x ":" ?x:v ":" (fact-index ?x) crlf))
  (printout t "done" crlf)
  (halt))
