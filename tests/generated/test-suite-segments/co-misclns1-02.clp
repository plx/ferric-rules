(deftemplate status 
   (field parent))
(deffacts initial-positions
  (status (parent nil)))
(defrule move-alone 
  ?node <- (status (parent nil))
  =>
  (duplicate ?node (parent ?node)))
