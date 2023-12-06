
# Prompt user for confirmation
read -p "Are you sure you want to proceed? (Type 'yes' to continue): " confirmation

# Check if the confirmation matches
if [ "$confirmation" != "yes" ]; then
    echo "Execution canceled."
    exit 0
fi


rm -rf storage

mkdir storage
cd storage
mkdir merkle_trees


cd ../../relay_server/src
rm ./orderBooks.db

# cd ../../../marketmaker/
# rm ./OrderBooks

echo "Storage was cleared, all files deleted..."








