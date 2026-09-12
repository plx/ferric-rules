; Explicit and omitted default slots produce the same template fact.
;; Level: boundary
;; Covers: facts, duplicate-template-defaults
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot code (default blue)) (slot count (default 3)))
(deffacts input (item) (item (count 3) (code blue)))
(defrule observe (item (code ?code)) => (printout t ?code crlf))
