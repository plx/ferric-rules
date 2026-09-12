; Duplicate suppresses a copy whose overridden values already exist.
;; Level: interaction
;; Covers: facts, duplicate-existing-suppressed
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot count))
(deffacts input (copy) (item (count 1)) (item (count 2)))
(defrule copy-item
  (declare (salience 10)) ?control <- (copy) ?f <- (item (count 1))
  => (retract ?control) (duplicate ?f (count 2)))
(defrule observe (item (count 2)) => (printout t "one copy" crlf))
