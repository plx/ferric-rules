(deffunction mark () (printout t "expanded;") (create$ a b))
(defrule run =>
 (printout nil (expand$ (mark)))
 (printout t (expand$ (create$ x y)) crlf)
 (bind ?values (expand$ (create$ a b)))
 (printout t ?values crlf))
