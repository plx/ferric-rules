(deftemplate nar 
   (slot bc))
(defrule migrant 
   (test (eq 1 1))
   (nar (bc ?bc))
   =>
   (printout t ?bc crlf))
(deffacts stuff
   (nar  (bc "US")))
