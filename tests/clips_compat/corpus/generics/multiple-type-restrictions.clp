;; A method restriction may accept more than one type.
;; Level: basic
;; Covers: generics, multiple-type-restrictions
(defgeneric classify)
(defmethod classify ((?x INTEGER STRING)) accepted)
(defrule probe => (printout t (classify 2) ":" (classify "two") crlf))
