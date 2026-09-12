; A fired activation does not fire again while its original facts remain.
;; Level: boundary
;; Covers: agenda, refraction
; Protocol: load, reset, run to quiescence.
(deffacts input (ready))
(defrule observe (ready) => (printout t "once" crlf) (assert (unrelated)))
