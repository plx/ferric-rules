;; INTEGER is more specific than NUMBER regardless of source order.
;; Level: boundary
;; Covers: generics, number-specificity
(defgeneric classify)
(defmethod classify ((?x INTEGER)) integer)
(defmethod classify ((?x NUMBER)) number)
(defrule probe => (printout t (classify 7) ":" (classify 7.5) crlf))
