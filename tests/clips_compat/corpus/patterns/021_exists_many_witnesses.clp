; Exists creates one activation despite multiple matching witnesses.
;; Level: boundary
;; Covers: patterns, exists-many-witnesses
; Protocol: load, reset, run to quiescence.
(deffacts input (item a) (item b) (item c))
(defrule observe (exists (item ?item)) => (printout t "exists" crlf))
