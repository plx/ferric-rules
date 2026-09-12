; Duplicate creates an overridden copy and keeps the original fact.
;; Level: interaction
;; Covers: facts, duplicate-preserves-original
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot name) (slot count))
(deffacts input (copy) (item (name blue) (count 1)))
(defrule copy-item
  ?control <- (copy) ?f <- (item (count 1))
  => (retract ?control) (duplicate ?f (count 2)))
(defrule observe
  (item (name ?name) (count 1)) (item (name ?name) (count 2))
  => (printout t ?name " original+copy" crlf))
