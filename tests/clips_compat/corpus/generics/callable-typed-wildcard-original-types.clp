(defgeneric classify)
(defmethod classify ($?r) (str-cat "fallback:" (length$ ?r)))
(defmethod classify (($?r INTEGER)) (str-cat "integer:" (length$ ?r)))
(defrule run =>
  (printout t "none:" (classify) crlf)
  (printout t "empty:" (classify (create$)) crlf)
  (printout t "scalars:" (classify 1 2) crlf)
  (printout t "multifield:" (classify (create$ 1 2)) crlf)
  (printout t "mixed:" (classify 1 (create$ 2 3)) crlf))
