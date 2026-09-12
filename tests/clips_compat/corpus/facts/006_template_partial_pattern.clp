; Unmentioned template slots do not restrict matching.
;; Level: basic
;; Covers: facts, template-partial-pattern
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot code) (slot count))
(deffacts input (item (code blue) (count 9)))
(defrule observe (item (code ?code)) => (printout t ?code crlf))
