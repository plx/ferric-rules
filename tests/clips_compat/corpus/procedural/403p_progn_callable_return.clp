(deffunction value ()
 (progn (printout t "before;") (bind ?x 7) (return ?x) (printout t "bad"))
 99)
(defrule run => (printout t (value) crlf))
