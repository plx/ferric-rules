(deftemplate p (slot v))
(deffacts seed (p (v 1)) (p (v 2)))
(defrule run =>
  (bind ?n 0)
  (do-for-all-facts ((?x p)) TRUE
    (bind ?n (+ ?n 1))
    (printout t "before:" ?n ":" ?x:v crlf)
    (reset)
    (printout t "after:" ?n ":" ?x:v crlf)
    (if (>= ?n 3) then (break)))
  (printout t "done:" ?n crlf)
  (halt))
