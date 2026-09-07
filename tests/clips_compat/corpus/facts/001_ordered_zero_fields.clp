; An ordered fact may have no fields.
;; Level: basic
;; Covers: facts, ordered-zero-fields
; Protocol: load, reset, run to quiescence.
(deffacts input (ready))
(defrule observe (ready) => (printout t "ready" crlf))
