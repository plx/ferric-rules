;; A defglobal literal is visible on the rule RHS.
;; Level: basic
;; Covers: modules, global-literal
(defglobal ?*answer* = 42)
(defrule probe => (printout t ?*answer* crlf))
