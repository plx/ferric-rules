(deffunction isolated () (if FALSE then (bind ?local 0)) ?local)
(defrule probe =>
  (bind ?local 777)
  (printout t (isolated) crlf)
  (printout t "after-error" crlf))
