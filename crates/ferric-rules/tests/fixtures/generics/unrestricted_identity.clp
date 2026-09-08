;; Issue #323: unrestricted defmethod parameter compatibility.
;; Original clips_compat/corpus/generics/bare-method-parameter.clp reproduction.
(defgeneric identity)

;; BEGIN METHODS
(defmethod identity (?x) ?x)
;; END METHODS

(defrule probe => (printout t (identity 17) crlf))
