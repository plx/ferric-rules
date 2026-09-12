;; Explicit method indices do not override type specificity.
;; Level: basic
;; Covers: generics, explicit-method-indices
(defgeneric classify)
(defmethod classify 20 ((?x NUMBER)) number)
(defmethod classify 10 ((?x INTEGER)) integer)
(defrule probe => (printout t (classify 5) ":" (classify 5.5) crlf))
