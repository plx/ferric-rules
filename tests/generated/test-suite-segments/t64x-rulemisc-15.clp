(deftemplate TAG2100
   (slot source)
   (slot matched)
   (slot sort-order))
(defrule load-data
   =>
   (assert (TAGS100)
           (TAG2100 (source ESI) (matched yes) (sort-order 2))
           (TAG2100 (source GCSS) (matched yes) (sort-order 19))))
(defrule Rule-2 ""
   
   (TAG2100 (source ESI)
            (matched ?m))
            
   (TAG2100 (source GCSS)
            (matched ?m)
            (sort-order ?so1))

   (not (and (TAGS100)
                       
             (not (TAG2100 (source GCSS)
                           (sort-order ?so5&:(< ?so5 ?so1))))))
   
   =>)
