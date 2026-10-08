(defglobal ?*round* = 0)
(deffacts d (a 1) (a 2))
(defrule ft (forall (a ?x) (test (> ?x 0))) =>
  (bind ?*round* (+ ?*round* 1))
  (printout t "all " ?*round* crlf)
  (if (= ?*round* 1) then (assert (a -1))))
(defrule repair ?f <- (a -1) => (retract ?f) (printout t repaired crlf))
