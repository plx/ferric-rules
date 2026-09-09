;; Run removal, optionally snapshot/restore, assert40/50, retract40, reassert40.
;; Install resume rule, assert observe, run; reset and run removal again.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*count* = 0)
(defrule remove_all (declare (salience 100)) =>
  (delayed-do-for-all-facts ((?f item)) TRUE
    (retract ?f) (bind ?*count* (+ ?*count* 1)))
  (printout t "removed:" ?*count* ":" (any-factp ((?f item)) TRUE) crlf))
;; RESUME AFTER MUTATION
(defrule observe_after ?marker <- (observe) (exists (item (value ?value))) =>
  (printout t "resumed:" (length$ (find-all-facts ((?f item)) TRUE)) ":")
  (do-for-all-facts ((?f item)) TRUE (printout t ?f:value ":"))
  (printout t crlf)
  (retract ?marker))
