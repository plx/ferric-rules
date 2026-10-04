(deftemplate p (slot v))
(defrule run =>
  (bind ?index 9)
  (printout t "modify:[" (modify ?index (v 1)) "] duplicate:[" (duplicate ?index (v 2)) "] continued" crlf))
