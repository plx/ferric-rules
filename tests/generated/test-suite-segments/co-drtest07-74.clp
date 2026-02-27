(deftemplate foo             ;; DR0692
  (multifield linkTagList))
(defrule foo
   ?w<-(foo)
   =>
   (modify ?w (linkTagList ?linktag ?linktagx ?a $?b)))
