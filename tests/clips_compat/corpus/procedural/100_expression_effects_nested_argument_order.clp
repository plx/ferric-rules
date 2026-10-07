(deftemplate p (slot v))
(deffunction create-one (?v) (printout t "create:" ?v ";") (assert (p (v ?v))))
(defrule run =>
  (printout t "prefix[" (create$ (create-one 1) (create-one 2)) "]suffix" crlf)
  (printout t "if:[" (if (assert (p (v 3))) then accepted else rejected) "]" crlf)
  (printout t (find-all-facts ((?f p)) TRUE) crlf))
