; A shared variable correlates facts across positive patterns.
;; Level: basic
;; Covers: patterns, join-shared-variable
; Protocol: load, reset, run to quiescence.
(deffacts input (left a) (left b) (right b) (right c))
(defrule observe (left ?value) (right ?value) => (printout t ?value crlf))
