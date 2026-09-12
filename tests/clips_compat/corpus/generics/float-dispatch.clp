;; A FLOAT argument selects its matching method.
;; Level: basic
;; Covers: generics, float-dispatch
(defgeneric classify)
(defmethod classify ((?x INTEGER)) integer)
(defmethod classify ((?x FLOAT)) float)
(defrule probe => (printout t (classify 7.5) crlf))
