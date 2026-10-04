(deffunction nop ())
(defgeneric nopm)
(defmethod nopm ((?x INTEGER)))
(defrule run =>
  (printout t "function:[" (nop) "] method:[" (nopm 1) "]" crlf)
  (printout t (eq (nop) FALSE) ":" (eq (nopm 1) FALSE) crlf)
  (printout t (create$ (nop) (nopm 1)) crlf))
