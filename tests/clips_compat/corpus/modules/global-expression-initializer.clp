;; A defglobal initializer evaluates a pure expression.
;; Level: basic
;; Covers: modules, global-expression-initializer
(defglobal ?*answer* = (+ 10 (* 2 3)))
(defrule probe => (printout t ?*answer* crlf))
