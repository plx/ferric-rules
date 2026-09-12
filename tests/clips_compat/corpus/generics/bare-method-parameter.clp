;; An unrestricted method parameter may use bare single-variable syntax.
;; Level: boundary
;; Covers: generics, bare-method-parameter
(defgeneric identity)
(defmethod identity (?x) ?x)
(defrule probe => (printout t (identity 17) crlf))
