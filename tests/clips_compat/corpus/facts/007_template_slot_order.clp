; Slot order in assertions and patterns does not change slot identity.
;; Level: basic
;; Covers: facts, template-slot-order
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot left) (slot right))
(deffacts input (item (right b) (left a)))
(defrule observe
  (item (right ?right) (left ?left))
  => (printout t ?left " " ?right crlf))
