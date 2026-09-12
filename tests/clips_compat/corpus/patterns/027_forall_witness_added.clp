; Asserting the last missing forall witness enables a rule.
;; Level: interaction
;; Covers: patterns, forall-witness-added
; Protocol: load, reset, run to quiescence.
(deffacts input (item a) (item b) (done a) (start))
(defrule complete
  (declare (salience 10)) (start) => (assert (done b)))
(defrule observe
  (forall (item ?item) (done ?item)) => (printout t "complete" crlf))
