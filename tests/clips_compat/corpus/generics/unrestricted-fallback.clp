;; An unrestricted method handles values without a specialized method.
;; Level: boundary
;; Covers: generics, unrestricted-fallback
(defgeneric classify)
(defmethod classify (?x) any)
(defmethod classify ((?x INTEGER)) integer)
(defrule probe => (printout t (classify 7) ":" (classify blue) crlf))
