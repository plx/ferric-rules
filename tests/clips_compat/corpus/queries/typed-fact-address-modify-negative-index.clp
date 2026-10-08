(deftemplate item (slot v))
(deffacts seed (item (v 1)))
(defrule r (item (v 1)) =>
  (bind ?i -1)
  (printout t "before" crlf)
  (modify ?i (v 2))
  (printout t "unexpected" crlf))
