;; An INTEGER argument selects its matching method.
;; Level: basic
;; Covers: generics, integer-dispatch
(defgeneric classify)
(defmethod classify ((?x INTEGER)) integer)
(defrule probe => (printout t (classify 7) crlf))
