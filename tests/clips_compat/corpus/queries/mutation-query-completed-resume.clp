;; Issue #327: after a delayed query removes every fact, fresh assertions,
;; a retraction and a reassertion are visible in assertion order.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*count* = 0)
(defrule remove_all (declare (salience 100)) =>
  (delayed-do-for-all-facts ((?f item)) TRUE
    (retract ?f) (bind ?*count* (+ ?*count* 1)))
  (printout t "removed:" ?*count* ":" (any-factp ((?f item)) TRUE) crlf))
(defrule refill (declare (salience 50)) =>
  (assert (item (value 40)) (item (value 50)) (reassert)))
(defrule reassert (declare (salience 40)) ?marker <- (reassert) ?f <- (item (value 40)) =>
  (retract ?marker ?f)
  (assert (item (value 40)) (observe)))
(defrule observe_after ?marker <- (observe) (exists (item (value ?value))) =>
  (printout t "resumed:" (length$ (find-all-facts ((?f item)) TRUE)) ":")
  (do-for-all-facts ((?f item)) TRUE (printout t ?f:value ":"))
  (printout t crlf)
  (retract ?marker))
