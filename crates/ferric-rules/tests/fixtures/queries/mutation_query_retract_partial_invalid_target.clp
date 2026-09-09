(deftemplate item (slot kind))
(deffacts seed (item (kind first)) (item (kind second)))
(deffunction invalid-target () wrong)
(defrule controller
  ?first <- (item (kind first))
  ?second <- (item (kind second))
  =>
  (retract ?first (invalid-target))
  (printout t "after-invalid" crlf))
